//! This module implements a shadowed orderbook, that is a main orderbook
//! retrieval method that gets shadowed by a secondary one, and where the result
//! is compared between the two.
//!
//! This is useful for validating alternate account retrieval methods during
//! development.

#![allow(dead_code)]

use super::StableXOrderBookReading;
use crate::models::{AccountState, Order, TokenId};
use anyhow::Result;
use ethcontract::{Address, U256};
use std::collections::{HashMap, HashSet};
use std::sync::mpsc::{self, Receiver, SyncSender};
use std::thread::{self, JoinHandle};

/// A type definition representing a complete orderbook.
type Orderbook = (AccountState, Vec<Order>);

/// A shadowed orderbook reader where two orderbook reading implementations
/// compare results.
pub struct ShadowedOrderbookReader<'a> {
    primary: &'a dyn StableXOrderBookReading,
    shadow_thread: JoinHandle<()>,
    shadow_channel: SyncSender<(u32, Orderbook)>,
}

impl<'a> ShadowedOrderbookReader<'a> {
    /// Create a new instance of a shadowed orderbook reader that starts a
    /// background thread
    pub fn new(
        primary: &'a dyn StableXOrderBookReading,
        shadow: Box<dyn StableXOrderBookReading + Send + 'static>,
    ) -> Self {
        // NOTE: Create a bounded channel with a 0-sized buffer, this makes it
        //   if the primary orderbook is read and the shadow is still reading,
        //   the diff for that specific orderbook is skipped.
        let (shadow_channel_tx, shadow_channel_rx) = mpsc::sync_channel(0);
        let shadow_thread =
            thread::spawn(move || background_shadow_reader(&*shadow, shadow_channel_rx));

        ShadowedOrderbookReader {
            primary,
            shadow_thread,
            shadow_channel: shadow_channel_tx,
        }
    }

    /// Eplicitely stop the shadowed orderbook reader returning an error if the
    /// inner shadow reader thread panicked.
    ///
    /// Note this method does not need to be called as the shadow thread will
    /// automatically get cleaned up once the reader is dropped.
    pub fn stop(self) -> thread::Result<()> {
        let Self {
            shadow_thread,
            shadow_channel,
            ..
        } = self;

        drop(shadow_channel);
        shadow_thread.join()?;

        Ok(())
    }
}

impl<'a> StableXOrderBookReading for ShadowedOrderbookReader<'a> {
    fn get_auction_data(&self, batch_id: U256) -> Result<Orderbook> {
        let orderbook = self.primary.get_auction_data(batch_id)?;

        // NOTE: Ignore errors here as they indicate that the shadow reader is
        //   already reading an orderbook.
        let _ = self
            .shadow_channel
            .try_send((batch_id.low_u32(), orderbook.clone()));

        Ok(orderbook)
    }
}

/// Background shadow thread that receives orders from the order channel,
/// queries the exact same account state with the shadow reader, and then
/// compares its results the ones from the primary reader.
///
/// Exits once the channel has been closed indicating that the shadow
/// thread should exit.
fn background_shadow_reader(
    reader: &dyn StableXOrderBookReading,
    channel: Receiver<(u32, Orderbook)>,
) {
    while let Ok((batch_id, orderbook)) = channel.recv() {
        let diff = match Diff::compare_to_reader(&orderbook, reader, batch_id) {
            Ok(diff) if diff.is_empty() => continue,
            Ok(diff) => diff,
            Err(err) => {
                log::error!(
                    "encountered an error reading the orderbook with the shadow reader: {:?}",
                    err
                );
                continue;
            }
        };

        let Diff(balance_changes, order_changes) = diff;
        for balance_change in balance_changes {
            log::error!(
                "user {:?} token {} primary balance of {} different than shadow {}",
                balance_change.user,
                balance_change.token.0,
                balance_change.primary,
                balance_change.shadow,
            );
        }
        for order_change in order_changes {
            match order_change {
                OrderChange::Added(order) => log::error!(
                    "user {:?} order {} in primary but missing from shadow",
                    order.account_id,
                    order.id,
                ),
                OrderChange::Removed(order) => log::error!(
                    "user {:?} order {} missing from primary but in shadow",
                    order.account_id,
                    order.id,
                ),
                OrderChange::Modified {
                    user,
                    id,
                    primary,
                    shadow,
                } => {
                    log::error!(
                        "user {:?} order {} with primary values {:?} but shadow values {:?}",
                        user,
                        id,
                        primary,
                        shadow,
                    );
                }
            }
        }
    }
}

/// A struct representing a diffs in two queried orderbooks.
#[derive(Debug, PartialEq)]
struct Diff(Vec<BalanceChange>, Vec<OrderChange>);

impl Diff {
    /// Compares the specified primary orderbook to a shadow orderbook.
    fn compare(primary: &Orderbook, shadow: &Orderbook) -> Self {
        Diff(
            BalanceChange::compare_account_state(&primary.0, &shadow.0),
            OrderChange::compare_orders(&primary.1, &shadow.1),
        )
    }

    /// Returns true if the diff is empty, in other words the primary and shadow
    /// orderbooks agree on all their data.
    fn is_empty(&self) -> bool {
        self.0.is_empty() && self.1.is_empty()
    }
}

/// Representation of a balance change between a primary and shadow orderbook.
#[derive(Debug, PartialEq)]
struct BalanceChange {
    user: Address,
    token: TokenId,
    primary: u128,
    shadow: u128,
}

impl BalanceChange {
    /// Compare a primary and shadow orderbook account state and return the
    /// changed balances.
    fn compare_account_state(primary: &AccountState, shadow: &AccountState) -> Vec<BalanceChange> {
        let user_token_pairs = primary
            .user_token_pairs()
            .chain(shadow.user_token_pairs())
            .collect::<HashSet<_>>();

        let mut changes = Vec::new();
        for (user, token_id) in user_token_pairs {
            let primary_balance = primary.read_balance(token_id, user);
            let shadow_balance = shadow.read_balance(token_id, user);
            if primary_balance != shadow_balance {
                changes.push(BalanceChange {
                    user,
                    token: token_id.into(),
                    primary: primary_balance,
                    shadow: shadow_balance,
                })
            }
        }

        changes
    }
}

/// Represents a change in order data between a primary and shadow orderbook.
#[derive(Debug, PartialEq)]
enum OrderChange {
    /// An order was added, i.e. it exists in the primary but not the shadow
    /// orderbook.
    Added(Order),
    /// An order was removed, i.e. it is missing from the primary orderbook but
    /// exists in shadow orderbook.
    Removed(Order),
    /// An order was modified, i.e. it exists in both orderbooks but with
    /// different values.
    Modified {
        user: Address,
        id: u16,
        primary: OrderValues,
        shadow: OrderValues,
    },
}

impl OrderChange {
    /// Compares a primary and shadow orderbook order vector and return the
    /// detected changes.
    fn compare_orders(primary: &[Order], shadow: &[Order]) -> Vec<OrderChange> {
        let mut shadow_orders = shadow
            .iter()
            .map(|order| ((order.account_id, order.id), order))
            .collect::<HashMap<_, _>>();

        let mut changes = Vec::new();
        for primary_order in primary {
            let (user, id) = (primary_order.account_id, primary_order.id);
            let change = match shadow_orders.remove(&(user, id)) {
                None => OrderChange::Added(primary_order.clone()),
                Some(shadow_order) if primary_order != shadow_order => OrderChange::Modified {
                    user,
                    id,
                    primary: OrderValues::from(primary_order),
                    shadow: OrderValues::from(shadow_order),
                },
                _ => continue,
            };
            changes.push(change);
        }

        for (_, remaining_shadow_order) in shadow_orders {
            changes.push(OrderChange::Removed(remaining_shadow_order.clone()))
        }

        changes
    }

    /// Retrieves the user and order ID for the order change.
    #[cfg(test)]
    fn user_and_id(&self) -> (Address, u16) {
        match self {
            OrderChange::Added(order) | OrderChange::Removed(order) => (order.account_id, order.id),
            OrderChange::Modified { user, id, .. } => (*user, *id),
        }
    }
}

/// Values that can possibly differ between orders.
#[derive(Debug, PartialEq)]
struct OrderValues {
    buy_token: u16,
    sell_token: u16,
    buy_amount: u128,
    sell_amount: u128,
}

impl From<&'_ Order> for OrderValues {
    fn from(order: &Order) -> Self {
        OrderValues {
            buy_token: order.buy_token,
            sell_token: order.sell_token,
            buy_amount: order.buy_amount,
            sell_amount: order.sell_amount,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn account_state_diff() {
        let addr = |i: u8| Address::repeat_byte(i);
        let mut diff = Diff::compare(
            &(
                AccountState(hash_map! {
                    (addr(1), 0) => 100,
                    (addr(1), 1) => 100,
                    (addr(3), 3) => 100,
                }),
                vec![
                    Order {
                        id: 0,
                        account_id: addr(1),
                        buy_token: 0,
                        sell_token: 1,
                        buy_amount: 100,
                        sell_amount: 100,
                    },
                    Order {
                        id: 1,
                        account_id: addr(1),
                        buy_token: 0,
                        sell_token: 1,
                        buy_amount: 100,
                        sell_amount: 100,
                    },
                    Order {
                        id: 0,
                        account_id: addr(2),
                        buy_token: 0,
                        sell_token: 1,
                        buy_amount: 100,
                        sell_amount: 100,
                    },
                ],
            ),
            &(
                AccountState(hash_map! {
                    (addr(1), 0) => 100,
                    (addr(2), 1) => 100,
                    (addr(3), 3) => 101,
                }),
                vec![
                    Order {
                        id: 0,
                        account_id: addr(1),
                        buy_token: 0,
                        sell_token: 1,
                        buy_amount: 100,
                        sell_amount: 100,
                    },
                    Order {
                        id: 1,
                        account_id: addr(1),
                        buy_token: 0,
                        sell_token: 1,
                        buy_amount: 100,
                        sell_amount: 101,
                    },
                    Order {
                        id: 1,
                        account_id: addr(42),
                        buy_token: 0,
                        sell_token: 1,
                        buy_amount: 100,
                        sell_amount: 100,
                    },
                ],
            ),
        );

        // NOTE: Order changes to make them easier to compare for testing, since
        //   we use hash maps and sets, the order is not deterministic.
        diff.0.sort_unstable_by_key(|b| (b.user, b.token));
        diff.1.sort_unstable_by_key(|o| o.user_and_id());

        assert_eq!(
            diff,
            Diff(
                vec![
                    BalanceChange {
                        user: addr(1),
                        token: TokenId(1),
                        primary: 100,
                        shadow: 0,
                    },
                    BalanceChange {
                        user: addr(2),
                        token: TokenId(1),
                        primary: 0,
                        shadow: 100,
                    },
                    BalanceChange {
                        user: addr(3),
                        token: TokenId(3),
                        primary: 100,
                        shadow: 101,
                    },
                ],
                vec![
                    OrderChange::Modified {
                        user: addr(1),
                        id: 1,
                        primary: OrderValues {
                            buy_token: 0,
                            sell_token: 1,
                            buy_amount: 100,
                            sell_amount: 100,
                        },
                        shadow: OrderValues {
                            buy_token: 0,
                            sell_token: 1,
                            buy_amount: 100,
                            sell_amount: 101,
                        },
                    },
                    OrderChange::Added(Order {
                        id: 0,
                        account_id: addr(2),
                        buy_token: 0,
                        sell_token: 1,
                        buy_amount: 100,
                        sell_amount: 100,
                    },),
                    OrderChange::Removed(Order {
                        id: 1,
                        account_id: addr(42),
                        buy_token: 0,
                        sell_token: 1,
                        buy_amount: 100,
                        sell_amount: 100,
                    },),
                ],
            )
        );
    }
}
