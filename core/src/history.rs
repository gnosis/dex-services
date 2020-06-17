//! This module contains an implementation for querying historic echange data by
//! inspecting indexed events.

use crate::{
    contracts::stablex_contract::batch_exchange,
    models::BatchId,
    models::{AccountState, Order},
    orderbook::streamed::{orderbook, State},
};
use anyhow::Result;
use std::{fs::File, io::Read, path::Path};

/// Historic exchange data.
pub struct ExchangeHistory {
    events: Vec<(batch_exchange::Event, BatchId)>,
}

impl ExchangeHistory {
    /// Reads historic exchange events from an `Orderbook` bincode
    /// representation.
    pub fn read(read: impl Read) -> Result<Self> {
        let orderbook = orderbook::Orderbook::read(read)?;
        let events = orderbook
            .into_events()
            .map(|(event, batch_id)| (event, BatchId(batch_id as _)))
            .collect();

        Ok(ExchangeHistory::from_events(events))
    }

    fn from_events(events: Vec<(batch_exchange::Event, BatchId)>) -> Self {
        // NOTE: Since history methods depend on events being sorted, assert
        // this invariant is true!
        debug_assert!(events.windows(2).all(|event_pair| {
            let (_, previous_event_batch) = event_pair[0];
            let (_, next_event_batch) = event_pair[1];
            previous_event_batch <= next_event_batch
        }));

        ExchangeHistory { events }
    }

    /// Reads historic exchange events from an `Orderbook` filestore.
    pub fn from_filestore(orderbook_filestore: impl AsRef<Path>) -> Result<Self> {
        ExchangeHistory::read(File::open(orderbook_filestore)?)
    }

    /// Returns an iterator of all events, including their batch IDs, that were
    /// emitted until the end of the specified batch.
    fn events_until_end_of_batch(
        &self,
        batch_id: impl Into<BatchId>,
    ) -> impl Iterator<Item = (&batch_exchange::Event, BatchId)> + '_ {
        let end_batch_id = batch_id.into();
        self.events
            .iter()
            .take_while(move |(_, batch_id)| *batch_id <= end_batch_id)
            .map(|(event, batch_id)| (event, *batch_id))
    }

    /// Returns the finalized orderbook at a historic batch.
    ///
    /// The finalized orderbook includes all account state changes and orders
    /// that are considered by the smart contract for solving the specified
    /// batch.
    pub fn auction_data_for_batch(
        &self,
        batch_id: impl Into<BatchId>,
    ) -> Result<(AccountState, Vec<Order>)> {
        let batch_id = batch_id.into();

        State::from_events(
            self.events_until_end_of_batch(batch_id)
                .map(|(event, batch_id)| (event, batch_id.into())),
        )?
        .normalized_auction_state_at_beginning_of_batch(batch_id.next().into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::contracts::stablex_contract::batch_exchange::{event_data::*, Event};
    use ethcontract::{Address, EventData, H256};
    use std::time::SystemTime;

    fn block_hash(block_number: u64) -> H256 {
        H256::from_low_u64_be(block_number)
    }

    fn batch_timestamp(batch_id: u32) -> u64 {
        BatchId(batch_id as _)
            .order_collection_start_time()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_secs()
    }

    fn user(user: u64) -> Address {
        Address::from_low_u64_be(user)
    }

    fn token(id: u16) -> Address {
        Address::from_low_u64_be(id as _)
    }

    fn token_listing(token_id: u16) -> batch_exchange::Event {
        Event::TokenListing(TokenListing {
            id: token_id,
            token: token(token_id),
        })
    }

    #[test]
    fn read_orderbook_filestore() {
        let orderbook_bincode = {
            let mut orderbook = orderbook::Orderbook::default();
            orderbook.handle_event_data(
                EventData::Added(token_listing(0)),
                1,
                0,
                block_hash(1),
                batch_timestamp(41),
            );
            orderbook.handle_event_data(
                EventData::Added(token_listing(1)),
                2,
                0,
                block_hash(2),
                batch_timestamp(41),
            );
            orderbook.handle_event_data(
                EventData::Added(token_listing(2)),
                2,
                1,
                block_hash(2),
                batch_timestamp(41),
            );
            orderbook.handle_event_data(
                EventData::Added(token_listing(3)),
                4,
                0,
                block_hash(4),
                batch_timestamp(42),
            );
            orderbook.to_bytes().unwrap()
        };

        let history = ExchangeHistory::read(&*orderbook_bincode).unwrap();
        assert_eq!(
            history.events,
            vec![
                (token_listing(0), BatchId(41)),
                (token_listing(1), BatchId(41)),
                (token_listing(2), BatchId(41)),
                (token_listing(3), BatchId(42)),
            ],
        );
    }

    #[test]
    fn iterates_events_until_batch() {
        let history = ExchangeHistory::from_events(vec![
            (token_listing(0), BatchId(0)),
            (token_listing(1), BatchId(1)),
            (token_listing(2), BatchId(2)),
        ]);
        let events = history.events_until_end_of_batch(1).collect::<Vec<_>>();

        assert_eq!(
            events,
            vec![
                (&token_listing(0), BatchId(0)),
                (&token_listing(1), BatchId(1)),
            ]
        );
    }

    #[test]
    fn computes_historic_orderbook() {
        let history = ExchangeHistory::from_events(vec![
            (token_listing(0), BatchId(0)),
            (token_listing(1), BatchId(0)),
            (
                Event::OrderPlacement(OrderPlacement {
                    owner: user(0),
                    index: 0,
                    buy_token: 1,
                    sell_token: 0,
                    price_numerator: 1_000_000,
                    price_denominator: 1_000_000,
                    valid_from: 1,
                    valid_until: u32::MAX,
                }),
                BatchId(0),
            ),
            (
                Event::Deposit(Deposit {
                    user: user(0),
                    token: token(0),
                    amount: 10_000_000.into(),
                    batch_id: 1,
                }),
                BatchId(1),
            ),
            (
                Event::Deposit(Deposit {
                    user: user(0),
                    token: token(0),
                    amount: 10_000_000.into(),
                    batch_id: 2,
                }),
                BatchId(2),
            ),
        ]);

        let auction_data = history.auction_data_for_batch(1).unwrap();
        assert_eq!(
            auction_data,
            (
                AccountState(hash_map! {
                    (user(0), 0) => 10_000_000.into(),
                }),
                vec![Order {
                    id: 0,
                    account_id: user(0),
                    buy_token: 1,
                    sell_token: 0,
                    numerator: 1_000_000,
                    denominator: 1_000_000,
                    remaining_sell_amount: 1_000_000,
                    valid_from: 1,
                    valid_until: u32::MAX,
                }],
            )
        );

        let (account_state, _) = history.auction_data_for_batch(2).unwrap();
        assert_eq!(account_state.read_balance(0, user(0)), 20_000_000.into());
    }
}
