//! This module contains an implementation for querying historic echange data by
//! inspecting indexed events.

pub mod batches;
pub mod events;

use self::batches::Batches;
use self::events::EventRegistry;
use crate::models::BatchId;
use anyhow::Result;
use contracts::batch_exchange::{
    event_data::{SolutionSubmission, Trade},
    Event,
};
use pricegraph::Element;
use std::{fs::File, io::Read, path::Path};

/// Historic exchange data.
pub struct ExchangeHistory {
    events: EventRegistry,
}

impl ExchangeHistory {
    /// Reads historic exchange events from an `EventRegistry` bincode
    /// representation.
    pub fn read(read: impl Read) -> Result<Self> {
        let events = EventRegistry::read(read)?;

        Ok(ExchangeHistory { events })
    }

    /// Reads historic exchange events from an `EventRegistry` filestore.
    pub fn from_filestore(filestore: impl AsRef<Path>) -> Result<Self> {
        ExchangeHistory::read(File::open(filestore)?)
    }

    /// Returns the very first batch for the exchange.
    pub fn first_batch(&self) -> Option<BatchId> {
        let (_, batch) = self.events.events().next()?;
        Some(batch)
    }

    /// Returns an iterator over all batches until the current
    pub fn batches_until_now(&self) -> Batches {
        match self.first_batch() {
            Some(start) => Batches::from_batch(start),
            None => Batches::empty(),
        }
    }

    /// Returns a collection of auction elements for the specified batch.
    pub fn auction_elements_for_batch(&self, batch: impl Into<BatchId>) -> Result<Vec<Element>> {
        let (accounts, orders) = self.events.auction_state_for_batch(batch)?;
        Ok(orders
            .into_iter()
            .map(|order| order.to_element_with_accounts(&accounts))
            .collect())
    }

    /// Returns a batch settlement information for the specified batch. Returns
    /// `None` if no solution was sumbitted for the specified batch.
    pub fn settlement_for_batch(&self, batch: impl Into<BatchId>) -> Option<Settlement> {
        // NOTE: Solution submission is done in the following batch.
        let events = self.events.events_for_batch(batch.into().next());

        let mut trades = Vec::new();
        let mut solution = None;
        for event in events {
            match event {
                Event::Trade(trade) => trades.push(trade.clone()),
                Event::TradeReversion(_) => {
                    trades.clear();
                    solution = None;
                }
                Event::SolutionSubmission(solution_submission) => {
                    solution = Some(solution_submission)
                }
                _ => {}
            }
        }

        let solution = solution?.clone();
        Some(Settlement { trades, solution })
    }
}

/// Batch settlement data including all final solution trades and prices.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Settlement {
    pub trades: Vec<Trade>,
    pub solution: SolutionSubmission,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::BatchId;
    use contracts::batch_exchange;
    use ethcontract::{Address, H256};

    fn block_hash(block_number: u64) -> H256 {
        H256::from_low_u64_be(block_number)
    }

    fn batch_timestamp(batch_id: impl Into<BatchId>) -> u64 {
        batch_id.into().as_timestamp()
    }

    fn token_listing(token: u16) -> Event {
        Event::TokenListing(batch_exchange::event_data::TokenListing {
            id: token,
            token: Address::from_low_u64_be(token as _),
        })
    }

    fn trade_as_reversion(trade: &Trade) -> batch_exchange::event_data::TradeReversion {
        batch_exchange::event_data::TradeReversion {
            owner: trade.owner,
            order_id: trade.order_id,
            sell_token: trade.sell_token,
            buy_token: trade.buy_token,
            executed_sell_amount: trade.executed_sell_amount,
            executed_buy_amount: trade.executed_buy_amount,
        }
    }

    #[test]
    fn read_event_filestore() {
        let bincode = {
            let mut events = EventRegistry::default();
            events.handle_event_data(token_listing(0), 1, 0, block_hash(1), batch_timestamp(41));
            events.handle_event_data(token_listing(1), 2, 0, block_hash(2), batch_timestamp(41));
            events.handle_event_data(token_listing(2), 2, 1, block_hash(2), batch_timestamp(41));
            events.handle_event_data(token_listing(3), 4, 0, block_hash(4), batch_timestamp(42));
            events.to_bytes().unwrap()
        };

        let history = ExchangeHistory::read(&*bincode).unwrap();
        assert_eq!(
            history.events.into_events().collect::<Vec<_>>(),
            vec![
                (token_listing(0), BatchId(41)),
                (token_listing(1), BatchId(41)),
                (token_listing(2), BatchId(41)),
                (token_listing(3), BatchId(42)),
            ],
        );
    }

    #[test]
    fn auction_settlement() {
        let batch = BatchId(42);
        let trades = vec![
            Trade {
                owner: Address::from_low_u64_be(1),
                order_id: 0,
                sell_token: 1,
                buy_token: 2,
                executed_sell_amount: 3,
                executed_buy_amount: 4,
            },
            Trade {
                owner: Address::from_low_u64_be(2),
                order_id: 0,
                sell_token: 2,
                buy_token: 1,
                executed_sell_amount: 4,
                executed_buy_amount: 3,
            },
        ];
        let solution = SolutionSubmission {
            submitter: Address::from_low_u64_be(0),
            utility: 1000.into(),
            disregarded_utility: 100.into(),
            burnt_fees: 1_000_000.into(),
            last_auction_burnt_fees: 0.into(),
            prices: vec![1, 2, 3],
            token_ids_for_price: vec![0, 1, 2],
        };

        let history = {
            let mut event_data = Vec::new();
            event_data.extend(trades.iter().map(|trade| Event::Trade(trade.clone())));
            event_data.push(Event::SolutionSubmission(solution.clone()));

            let mut events = EventRegistry::default();
            for (i, event) in event_data.into_iter().enumerate() {
                events.handle_event_data(
                    event,
                    1337,
                    i,
                    block_hash(0),
                    batch.next().as_timestamp(),
                );
            }

            ExchangeHistory { events }
        };

        assert_eq!(
            history.settlement_for_batch(batch).unwrap(),
            Settlement { trades, solution },
        );
    }

    #[test]
    fn auction_settlement_with_reversion() {
        let batch = BatchId(42);
        let trades_0 = vec![
            Trade {
                owner: Address::from_low_u64_be(1),
                order_id: 0,
                sell_token: 1,
                buy_token: 2,
                executed_sell_amount: 3,
                executed_buy_amount: 4,
            },
            Trade {
                owner: Address::from_low_u64_be(2),
                order_id: 0,
                sell_token: 2,
                buy_token: 1,
                executed_sell_amount: 4,
                executed_buy_amount: 3,
            },
        ];
        let solution_0 = SolutionSubmission {
            submitter: Address::from_low_u64_be(0),
            utility: 1000.into(),
            disregarded_utility: 100.into(),
            burnt_fees: 1_000_000.into(),
            last_auction_burnt_fees: 0.into(),
            prices: vec![1, 2, 3],
            token_ids_for_price: vec![0, 1, 2],
        };

        let trades_1 = vec![
            Trade {
                owner: Address::from_low_u64_be(3),
                order_id: 0,
                sell_token: 3,
                buy_token: 4,
                executed_sell_amount: 5,
                executed_buy_amount: 6,
            },
            Trade {
                owner: Address::from_low_u64_be(2),
                order_id: 0,
                sell_token: 4,
                buy_token: 3,
                executed_sell_amount: 6,
                executed_buy_amount: 5,
            },
        ];
        let solution_1 = SolutionSubmission {
            submitter: Address::from_low_u64_be(422),
            utility: 1000.into(),
            disregarded_utility: 100.into(),
            burnt_fees: 1_000_000.into(),
            last_auction_burnt_fees: 0.into(),
            prices: vec![1, 4, 5],
            token_ids_for_price: vec![0, 3, 4],
        };

        let history = {
            let mut event_data = Vec::new();
            event_data.extend(trades_0.iter().map(|trade| Event::Trade(trade.clone())));
            event_data.push(Event::SolutionSubmission(solution_0));
            event_data.push(token_listing(42));
            event_data.extend(
                trades_0
                    .iter()
                    .map(|trade| Event::TradeReversion(trade_as_reversion(trade))),
            );
            event_data.extend(trades_1.iter().map(|trade| Event::Trade(trade.clone())));
            event_data.push(Event::SolutionSubmission(solution_1.clone()));

            let mut events = EventRegistry::default();
            for (i, event) in event_data.into_iter().enumerate() {
                events.handle_event_data(
                    event,
                    1337,
                    i,
                    block_hash(0),
                    batch.next().as_timestamp(),
                );
            }

            ExchangeHistory { events }
        };

        assert_eq!(
            history.settlement_for_batch(batch).unwrap(),
            Settlement {
                trades: trades_1,
                solution: solution_1
            },
        );
    }

    #[test]
    fn auction_settlement_for_without_solution() {
        let history = ExchangeHistory {
            events: EventRegistry::default(),
        };
        assert_eq!(history.settlement_for_batch(1337), None);
    }
}
