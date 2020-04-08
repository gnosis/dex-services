use super::*;
use crate::contracts::{
    stablex_auction_element,
    stablex_contract::batch_exchange::{event_data::*, Event},
};
use crate::models::Order as ModelOrder;
use balance::Balance;
use error::Error;
use order::Order;
use serde::{Deserialize, Serialize};
use std::cmp::Ordering;
use std::collections::hash_map::Entry;
use std::collections::HashMap;
use std::iter::Iterator;

// Most types, fields, functions in this module mirror the smart contract because we need to
// emulate what it does based on the events it emits.
//
// We keep track of the pending solution for the batch separately instead of applying the trades
// immediately. This ensures that orders and balances have correctly *not* been updated by the
// pending solution until the solution becomes the real solution.
// This has the side effect of simplifying solution revert logic by allowing us to replace the whole
// pending solution instead of having to revert individual trades.

/// The orderbook state as built from received events.
///
/// Note that there is no way to revert an event. The order in which events are received matters
/// Applying events `A B` does not always result in the same state as `B A`. For example, a Trade
/// can only be processed if the OrderPlacement for the traded order has previously been observed.
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct State {
    orders: HashMap<(UserId, OrderId), Order>,
    balances: HashMap<(UserId, TokenAddress), Balance>,
    tokens: Tokens,
    // TODO?: is_inconsistent_because_solution_not_fully_received: bool
    last_solution: LastSolution,
}

impl State {
    /// Can be used to create a `crate::models::AccountState`.
    pub fn account_state(
        &self,
        batch_id: BatchId,
    ) -> impl Iterator<Item = ((UserId, TokenId), u128)> + '_ {
        let tokens = &self.tokens;
        let extra_balance_from_last_solution = if dbg!(self.last_solution.batch_id) < batch_id {
            Some((
                self.last_solution.submitter,
                dbg!(self.last_solution.burnt_fees),
            ))
        } else {
            None
        };
        self.balances
            .iter()
            .filter_map(move |((user_id, token_address), balance)| {
                // It is possible that a user has a balance for a token that hasn't been added to
                // the exchange because tokens can be deposited anyway.
                let token_id = tokens.get_id_by_address(*token_address)?;
                let extra_balance = match extra_balance_from_last_solution {
                    Some((submitter, burnt_fees)) if dbg!(submitter) == dbg!(*user_id) => {
                        burnt_fees
                    }
                    _ => 0.into(),
                };
                Some((
                    (*user_id, token_id),
                    // TODO unwrap
                    (balance.get_balance(batch_id).unwrap() + extra_balance).low_u128(),
                ))
            })
    }

    pub fn orders(&self, batch_id: BatchId) -> impl Iterator<Item = ModelOrder> + '_ {
        self.orders
            .iter()
            .filter(move |(_, order)| order.is_valid_in_batch(batch_id))
            .map(move |((user_id, order_id), order)| {
                let (buy_amount, sell_amount) = stablex_auction_element::compute_buy_sell_amounts(
                    order.price_numerator,
                    order.price_denominator,
                    // TODO unwrap
                    order.price_denominator - order.get_used_amount(batch_id).unwrap(),
                );
                ModelOrder {
                    id: *order_id,
                    account_id: *user_id,
                    buy_token: order.buy_token,
                    sell_token: order.sell_token,
                    buy_amount,
                    sell_amount,
                }
            })
    }

    /// Reset the state to the default state in which no events have been applied.
    pub fn clear(&mut self) {
        self.orders.clear();
        self.balances.clear();
        self.tokens.0.clear();
    }

    /// Apply an event to the state, modifying it.
    ///
    /// In case of error no modifications take place.
    ///
    /// `block_batch_id` is the current batch based on the timestamp of the block that contains the
    ///  event.
    pub fn apply_event(&mut self, event: &Event, block_batch_id: BatchId) -> Result<(), Error> {
        match event {
            Event::Deposit(event) => self.apply_deposit(event, block_batch_id),
            Event::WithdrawRequest(event) => self.apply_withdraw_request(event),
            Event::Withdraw(event) => self.apply_withdraw(event, block_batch_id),
            Event::TokenListing(event) => self.apply_token_listing(event),
            Event::OrderPlacement(event) => self.apply_order_placement(event),
            Event::OrderCancellation(event) => self.apply_order_cancellation(event, block_batch_id),
            Event::OrderDeletion(event) => self.apply_order_deletion(event, block_batch_id),
            Event::Trade(event) => self.apply_trade(event, block_batch_id),
            Event::TradeReversion(event) => self.apply_trade_reversion(event, block_batch_id),
            Event::SolutionSubmission(event) => {
                self.apply_solution_submission(event, block_batch_id)
            }
        }
    }

    fn apply_deposit(&mut self, event: &Deposit, block_batch_id: BatchId) -> Result<(), Error> {
        let balance = self.balances.entry((event.user, event.token)).or_default();
        balance.deposit(event.amount, event.batch_id, block_batch_id)
    }

    fn apply_withdraw_request(&mut self, event: &WithdrawRequest) -> Result<(), Error> {
        let balance = self.balances.entry((event.user, event.token)).or_default();
        balance.withdraw_request(event.amount, event.batch_id);
        Ok(())
    }

    fn apply_withdraw(&mut self, event: &Withdraw, block_batch_id: BatchId) -> Result<(), Error> {
        let balance = self.balances.entry((event.user, event.token)).or_default();
        balance.withdraw(event.amount, block_batch_id)
    }

    fn apply_token_listing(&mut self, event: &TokenListing) -> Result<(), Error> {
        self.tokens.0.push((event.id, event.token));
        Ok(())
    }

    fn apply_order_placement(&mut self, event: &OrderPlacement) -> Result<(), Error> {
        let order = Order::new(
            event.buy_token,
            event.sell_token,
            event.valid_from,
            event.valid_until,
            event.price_numerator,
            event.price_denominator,
        );
        match self.orders.entry((event.owner, event.index)) {
            Entry::Vacant(entry) => entry.insert(order),
            Entry::Occupied(_) => return Err(Error::OrderAlreadyExists),
        };
        Ok(())
    }

    fn apply_order_cancellation(
        &mut self,
        event: &OrderCancellation,
        block_batch_id: BatchId,
    ) -> Result<(), Error> {
        let order = self
            .orders
            .get_mut(&(event.owner, event.id))
            .ok_or(Error::UnknownOrder(event.id))?;
        order.valid_until = block_batch_id - 1;
        Ok(())
    }

    fn apply_order_deletion(
        &mut self,
        event: &OrderDeletion,
        block_batch_id: BatchId,
    ) -> Result<(), Error> {
        if let Some(order) = self.orders.get(&(event.owner, event.id)) {
            if order.is_valid_in_batch(block_batch_id - 1) {
                return Err(Error::DeletingValidOrder);
            } else {
                self.orders.remove(&(event.owner, event.id));
            }
        }
        // Orders are allowed to be deleted multiple times.
        Ok(())
    }

    fn apply_trade(&mut self, event: &Trade, block_batch_id: BatchId) -> Result<(), Error> {
        let order = self
            .orders
            .get_mut(&(event.owner, event.order_id))
            .ok_or(Error::UnknownOrder(event.order_id))?;
        let sell_token = self
            .tokens
            .get_address_by_id(order.sell_token)
            .ok_or(Error::UnknownToken(order.sell_token))?;
        let buy_token = self
            .tokens
            .get_address_by_id(order.buy_token)
            .ok_or(Error::UnknownToken(order.buy_token))?;

        // This looks awkward because we make sure that no modifications take place if an error
        // occurs.

        let mut new_order = *order;
        let sell_balance_key = (event.owner, sell_token);
        let mut sell_balance = self
            .balances
            .get(&sell_balance_key)
            .cloned()
            .unwrap_or_default();
        let buy_balance_key = (event.owner, buy_token);
        let mut buy_balance = self
            .balances
            .get(&buy_balance_key)
            .cloned()
            .unwrap_or_default();

        new_order.trade(event.executed_sell_amount, block_batch_id)?;
        sell_balance.sell(event.executed_sell_amount, block_batch_id)?;
        buy_balance.buy(event.executed_buy_amount, block_batch_id)?;

        *order = new_order;
        self.balances.insert(sell_balance_key, sell_balance);
        self.balances.insert(buy_balance_key, buy_balance);

        Ok(())
    }

    fn apply_trade_reversion(
        &mut self,
        event: &TradeReversion,
        block_batch_id: BatchId,
    ) -> Result<(), Error> {
        let order = self
            .orders
            .get_mut(&(event.owner, event.order_id))
            .ok_or(Error::UnknownOrder(event.order_id))?;
        let sell_token = self
            .tokens
            .get_address_by_id(order.sell_token)
            .ok_or(Error::UnknownToken(order.sell_token))?;
        let buy_token = self
            .tokens
            .get_address_by_id(order.buy_token)
            .ok_or(Error::UnknownToken(order.buy_token))?;

        let mut new_order = *order;
        let sell_balance_key = (event.owner, sell_token);
        let mut sell_balance = *self
            .balances
            .get(&sell_balance_key)
            .ok_or(Error::RevertingNonExistentTrade)?;
        let buy_balance_key = (event.owner, buy_token);
        let mut buy_balance = *self
            .balances
            .get(&buy_balance_key)
            .ok_or(Error::RevertingNonExistentTrade)?;

        new_order.trade(event.executed_sell_amount, block_batch_id)?;
        sell_balance.revert_sell(event.executed_sell_amount, block_batch_id)?;
        buy_balance.buy(event.executed_buy_amount, block_batch_id)?;

        *order = new_order;
        self.balances.insert(sell_balance_key, sell_balance);
        self.balances.insert(buy_balance_key, buy_balance);

        Ok(())
    }

    fn apply_solution_submission(
        &mut self,
        event: &SolutionSubmission,
        block_batch_id: BatchId,
    ) -> Result<(), Error> {
        let fee_token = self
            .tokens
            .get_address_by_id(0)
            .ok_or(Error::SolutionWithoutFeeToken)?;
        match self.last_solution.batch_id.cmp(&block_batch_id) {
            Ordering::Less => {
                let balance = self
                    .balances
                    .entry((self.last_solution.submitter, fee_token))
                    .or_default();
                balance.add_balance(self.last_solution.burnt_fees)?;
            }
            Ordering::Equal => (),
            Ordering::Greater => return Err(Error::SolutionForPastBatch),
        }

        // Make sure that the submitter gets an entry an balances so their balance shows up when
        // place an order for the fee token. Otherwise this would not happen because we only create
        // balances on trades.
        self.balances
            .entry((event.submitter, fee_token))
            .or_default();

        self.last_solution.batch_id = block_batch_id;
        self.last_solution.submitter = event.submitter;
        self.last_solution.burnt_fees = event.burnt_fees;

        Ok(())
    }
}

/// Bidirectional map between token id and token address.
///
/// Std does not have this type so we use a vector. Alternatively we could find a crate.
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
struct Tokens(Vec<(TokenId, TokenAddress)>);

impl Tokens {
    fn get_address_by_id(&self, id: TokenId) -> Option<Address> {
        self.0
            .iter()
            .find(|(id_, _)| *id_ == id)
            .map(|(_, address)| *address)
    }

    fn get_id_by_address(&self, address: TokenAddress) -> Option<TokenId> {
        self.0
            .iter()
            .find(|(_, address_)| *address_ == address)
            .map(|(token_id, _)| *token_id)
    }
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
struct LastSolution {
    batch_id: BatchId,
    submitter: UserId,
    burnt_fees: U256,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::AccountState;

    fn address(n: u64) -> Address {
        Address::from_low_u64_be(n)
    }

    fn state_with_fee() -> State {
        let mut state = State::default();
        let event = TokenListing {
            token: address(0),
            id: 0,
        };
        state.apply_event(&Event::TokenListing(event), 0).unwrap();
        state
    }

    fn account_state(state: &State, batch_id: BatchId) -> AccountState {
        AccountState(state.account_state(batch_id).collect())
    }

    #[test]
    fn account_state_respects_deposit_batch() {
        let mut state = state_with_fee();
        let event = Deposit {
            user: address(3),
            token: address(0),
            amount: 1.into(),
            batch_id: 0,
        };
        state.apply_event(&Event::Deposit(event), 0).unwrap();
        let account_state_ = account_state(&state, 0);
        assert_eq!(account_state_.read_balance(0, address(3)), 0);
        let account_state_ = account_state(&state, 1);
        assert_eq!(account_state_.read_balance(0, address(3)), 1);
    }

    #[test]
    fn account_state_works_with_unlisted_token() {
        let mut state = state_with_fee();
        // token id 1 is not listed
        for token_id in 0..2 {
            let event = Deposit {
                user: address(3),
                token: address(token_id),
                amount: 1.into(),
                batch_id: 0,
            };
            state.apply_event(&Event::Deposit(event), 0).unwrap();
        }

        let account_state_ = account_state(&state, 1);
        assert_eq!(account_state_.read_balance(0, address(3)), 1);

        let event = TokenListing {
            token: address(1),
            id: 1,
        };
        state.apply_event(&Event::TokenListing(event), 0).unwrap();

        let account_state_ = account_state(&state, 1);
        assert_eq!(account_state_.read_balance(0, address(3)), 1);
        assert_eq!(account_state_.read_balance(1, address(3)), 1);
    }

    #[test]
    fn multiple_deposits_in_different_batches() {
        let mut state = state_with_fee();
        for i in 0..3 {
            let event = Deposit {
                user: address(1),
                token: address(0),
                amount: 1.into(),
                batch_id: i + 1,
            };
            state.apply_event(&Event::Deposit(event), i).unwrap();

            let account_state_ = account_state(&state, i + 2);
            assert_eq!(account_state_.read_balance(0, address(1)), i as u128 + 1);
        }
    }

    #[test]
    fn multiple_deposits_in_same_batch() {
        let mut state = state_with_fee();
        for i in 0..3 {
            let event = Deposit {
                user: address(1),
                token: address(0),
                amount: 1.into(),
                batch_id: 1,
            };
            state.apply_event(&Event::Deposit(event), 0).unwrap();

            let account_state_ = account_state(&state, 1);
            assert_eq!(account_state_.read_balance(0, address(1)), 0);
            let account_state_ = account_state(&state, 2);
            assert_eq!(account_state_.read_balance(0, address(1)), i + 1);
        }
    }

    #[test]
    fn withdraw_request_deducted_from_balance() {
        let mut state = state_with_fee();
        let event = Deposit {
            user: address(1),
            token: address(0),
            amount: 2.into(),
            batch_id: 0,
        };
        state.apply_event(&Event::Deposit(event), 0).unwrap();
        let event = WithdrawRequest {
            user: address(1),
            token: address(0),
            amount: 1.into(),
            batch_id: 0,
        };
        state
            .apply_event(&Event::WithdrawRequest(event), 0)
            .unwrap();
        let account_state_ = account_state(&state, 1);
        assert_eq!(account_state_.read_balance(0, address(1)), 1);
    }

    #[test]
    fn withdraw_request_does_not_underflow() {
        let mut state = state_with_fee();
        let event = Deposit {
            user: address(1),
            token: address(0),
            amount: 2.into(),
            batch_id: 0,
        };
        state.apply_event(&Event::Deposit(event), 0).unwrap();
        let event = WithdrawRequest {
            user: address(1),
            token: address(0),
            amount: 3.into(),
            batch_id: 2,
        };
        state
            .apply_event(&Event::WithdrawRequest(event), 1)
            .unwrap();
        let account_state_ = account_state(&state, 3);
        assert_eq!(account_state_.read_balance(0, address(1)), 0);
    }

    #[test]
    fn withdraw_deducted_from_balance() {
        let mut state = state_with_fee();
        let event = Deposit {
            user: address(1),
            token: address(0),
            amount: 2.into(),
            batch_id: 0,
        };
        state.apply_event(&Event::Deposit(event), 0).unwrap();
        let event = WithdrawRequest {
            user: address(1),
            token: address(0),
            amount: 2.into(),
            batch_id: 0,
        };
        state
            .apply_event(&Event::WithdrawRequest(event), 0)
            .unwrap();
        let event = Withdraw {
            user: address(1),
            token: address(0),
            amount: 1.into(),
        };
        state.apply_event(&Event::Withdraw(event), 1).unwrap();
        let account_state_ = account_state(&state, 1);
        assert_eq!(account_state_.read_balance(0, address(1)), 1);
    }

    #[test]
    fn withdraw_removes_pending_withdraw() {
        let mut state = state_with_fee();
        let event = Deposit {
            user: address(1),
            token: address(0),
            amount: 3.into(),
            batch_id: 0,
        };
        state.apply_event(&Event::Deposit(event), 0).unwrap();
        let event = WithdrawRequest {
            user: address(1),
            token: address(0),
            amount: 2.into(),
            batch_id: 0,
        };
        state
            .apply_event(&Event::WithdrawRequest(event), 1)
            .unwrap();
        let account_state_ = account_state(&state, 1);
        assert_eq!(account_state_.read_balance(0, address(1)), 1);
        let event = Withdraw {
            user: address(1),
            token: address(0),
            amount: 1.into(),
        };
        state.apply_event(&Event::Withdraw(event), 1).unwrap();
        let account_state_ = account_state(&state, 2);
        assert_eq!(account_state_.read_balance(0, address(1)), 2);
    }

    #[test]
    fn order_placement_cancellation_deletion() {
        let mut state = state_with_fee();
        assert_eq!(state.orders(0).next(), None);
        let event = OrderPlacement {
            owner: address(2),
            index: 0,
            buy_token: 0,
            sell_token: 1,
            valid_from: 1,
            valid_until: 2,
            price_numerator: 3,
            price_denominator: 4,
        };
        state.apply_event(&Event::OrderPlacement(event), 0).unwrap();

        assert_eq!(state.orders(0).next(), None);
        let expected_orders = vec![ModelOrder {
            id: 0,
            account_id: address(2),
            buy_token: 0,
            sell_token: 1,
            buy_amount: 3,
            sell_amount: 4,
        }];
        assert_eq!(state.orders(1).collect::<Vec<_>>(), expected_orders);
        assert_eq!(state.orders(2).collect::<Vec<_>>(), expected_orders);
        assert_eq!(state.orders(3).next(), None);

        let event = OrderCancellation {
            owner: address(2),
            id: 0,
        };
        state
            .apply_event(&Event::OrderCancellation(event), 2)
            .unwrap();

        assert_eq!(state.orders(0).next(), None);
        assert_eq!(state.orders(1).collect::<Vec<_>>(), expected_orders);
        assert_eq!(state.orders(2).next(), None);
        assert_eq!(state.orders(3).next(), None);

        let event = Event::OrderDeletion(OrderDeletion {
            owner: address(2),
            id: 0,
        });
        assert!(state.apply_event(&event, 2).is_err());
        state.apply_event(&event, 3).unwrap();
        assert_eq!(state.orders(0).next(), None);
        assert_eq!(state.orders(1).next(), None);
        assert_eq!(state.orders(2).next(), None);
        assert_eq!(state.orders(3).next(), None);
    }

    #[test]
    fn trade_and_reversion() {
        let mut state = state_with_fee();
        let event = TokenListing {
            token: address(1),
            id: 1,
        };
        state.apply_event(&Event::TokenListing(event), 0).unwrap();

        for token in 0..2 {
            for user in 2..4 {
                let event = Deposit {
                    user: address(user),
                    token: address(token),
                    amount: 10.into(),
                    batch_id: 0,
                };
                state.apply_event(&Event::Deposit(event), 0).unwrap();
            }
        }

        let event = OrderPlacement {
            owner: address(2),
            index: 0,
            buy_token: 0,
            sell_token: 1,
            valid_from: 0,
            valid_until: 10,
            price_numerator: 5,
            price_denominator: 5,
        };
        state.apply_event(&Event::OrderPlacement(event), 0).unwrap();
        let event = OrderPlacement {
            owner: address(3),
            index: 0,
            buy_token: 1,
            sell_token: 0,
            valid_from: 0,
            valid_until: 10,
            price_numerator: 3,
            price_denominator: 3,
        };
        state.apply_event(&Event::OrderPlacement(event), 0).unwrap();

        let event = Trade {
            owner: address(2),
            order_id: 0,
            executed_sell_amount: 1,
            executed_buy_amount: 2,
            ..Default::default()
        };
        state.apply_event(&Event::Trade(event), 1).unwrap();
        let event = Trade {
            owner: address(3),
            order_id: 0,
            executed_sell_amount: 2,
            executed_buy_amount: 1,
            ..Default::default()
        };
        state.apply_event(&Event::Trade(event), 1).unwrap();
        state
            .apply_event(
                &Event::SolutionSubmission(SolutionSubmission {
                    submitter: address(4),
                    burnt_fees: 42.into(),
                    ..Default::default()
                }),
                1,
            )
            .unwrap();

        let account_state_ = account_state(&state, 2);
        assert_eq!(account_state_.read_balance(0, address(2)), 12);
        assert_eq!(account_state_.read_balance(1, address(2)), 9);
        assert_eq!(account_state_.read_balance(0, address(3)), 8);
        assert_eq!(account_state_.read_balance(1, address(3)), 11);
        assert_eq!(account_state_.read_balance(0, address(4)), 42);
        assert_eq!(
            state
                .orders
                .get(&(address(2), 0))
                .unwrap()
                .get_used_amount(2)
                .unwrap(),
            1
        );
        assert_eq!(
            state
                .orders
                .get(&(address(3), 0))
                .unwrap()
                .get_used_amount(2)
                .unwrap(),
            2
        );
    }
}
