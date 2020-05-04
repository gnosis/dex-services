use super::{
    state::{Batch, State},
    BatchId,
};
use crate::{
    contracts::stablex_contract::batch_exchange,
    models::{AccountState, Order},
    orderbook::{util, StableXOrderBookReading},
};
use anyhow::{Context, Result};
use ethcontract::{contract::EventData, H256, U256};
use log::info;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::convert::TryFrom;
use std::fs;
use std::fs::File;
use std::io::{Read, Write};
use std::path::Path;

// Ethereum events (logs) can be both created and removed. Removals happen if the chain reorganizes
// and ends up not including block that was previously thought to be part of the chain.
// However, the orderbook state (`State`) cannot remove events. To support this, we keep an ordered
// list of all events based on which the state is built.

#[derive(Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
struct EventSortKey {
    block_number: u64,
    /// Is included to differentiate events from the same block number but different blocks which
    /// can happen during reorgs.
    block_hash: H256,
    log_index: usize,
}

#[derive(Debug, Deserialize, Eq, PartialEq, Serialize)]
struct Value {
    event: batch_exchange::Event,
    /// The batch id is calculated based on the timestamp of the block.
    batch_id: BatchId,
}

#[derive(Debug, Default, Deserialize, Serialize)]
pub struct Orderbook {
    events: BTreeMap<EventSortKey, Value>,
}

impl Orderbook {
    pub fn handle_event_data(
        &mut self,
        event_data: EventData<batch_exchange::Event>,
        block_number: u64,
        log_index: usize,
        block_hash: H256,
        block_timestamp: u64,
    ) {
        let batch_id = block_timestamp as BatchId / 300;
        let key = EventSortKey {
            block_number,
            block_hash,
            log_index,
        };
        match event_data {
            EventData::Added(event) => self.events.insert(key, Value { event, batch_id }),
            EventData::Removed(_event) => self.events.remove(&key),
        };
    }

    pub fn delete_events_starting_at_block(&mut self, block_number: u64) {
        self.events.split_off(&EventSortKey {
            block_number,
            block_hash: H256::zero(),
            log_index: 0,
        });
    }

    pub fn write_to_file(&self, path: impl AsRef<Path>) -> Result<()> {
        // Write to tmp file until complete and then rename.
        let temp_path = path.as_ref().with_extension(".temp");

        // Create temp file to be written completely before rename
        let mut temp_file = File::create(&temp_path)
            .with_context(|| format!("couldn't create {}", temp_path.display()))?;
        let file_content = bincode::serialize(self)?;
        temp_file.write_all(file_content.as_ref())?;

        // Rename the temp file to the originally specified path.
        fs::rename(temp_path, path)?;
        Ok(())
    }

    pub fn last_handled_block(&self) -> Option<u64> {
        Some(self.events.iter().next_back()?.0.block_number)
    }

    fn create_state(&self) -> Result<State> {
        self.events
            .iter()
            .try_fold(State::default(), |state, (_key, value)| {
                state.apply_event(&value.event, value.batch_id)
            })
    }
}

impl TryFrom<&[u8]> for Orderbook {
    type Error = anyhow::Error;

    fn try_from(bytes: &[u8]) -> Result<Self> {
        bincode::deserialize(bytes).context("Failed to load orderbook from bytes")
    }
}

impl TryFrom<File> for Orderbook {
    type Error = anyhow::Error;

    fn try_from(mut file: File) -> Result<Self> {
        let mut contents = Vec::new();
        let bytes_read = file
            .read_to_end(&mut contents)
            .with_context(|| format!("Failed to read file: {:?}", file))?;
        info!(
            "Successfully loaded {} bytes from Orderbook file",
            bytes_read
        );
        Orderbook::try_from(contents.as_slice())
    }
}

impl TryFrom<&Path> for Orderbook {
    type Error = anyhow::Error;

    fn try_from(path: &Path) -> Result<Self> {
        let file = File::open(path).with_context(|| format!("couldn't open {}", path.display()))?;
        Orderbook::try_from(file)
    }
}

impl StableXOrderBookReading for Orderbook {
    fn get_auction_data(&self, batch_id_to_solve: U256) -> Result<(AccountState, Vec<Order>)> {
        // TODO: Handle future batch ids for when we want to do optimistic solving.
        let state = self.create_state()?;
        // `orderbook_for_batch` takes the index of the auction that is currently collecting orders and returns
        // the orderbook for the batch index that is currently being solved. `get_auction_data` passed in the
        // index for the auction that orders should be valid for (the one currently being solved). Thus we need
        // to increment it.
        let (account_state, orders) =
            state.orderbook_for_batch(Batch::Future(batch_id_to_solve.low_u32() + 1))?;
        let (account_state, orders) = util::normalize_auction_data(
            account_state.map(|(key, balance)| (key, balance.low_u128())),
            orders,
        );
        Ok((account_state, orders))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::contracts::stablex_contract::batch_exchange::event_data::*;
    use crate::contracts::stablex_contract::batch_exchange::Event;
    use ethcontract::Address;

    #[test]
    fn test_serialize_deserialize_orderbook() {
        let event_list: Vec<Event> = vec![
            Event::Deposit(Deposit::default()),
            Event::WithdrawRequest(WithdrawRequest::default()),
            Event::Withdraw(Withdraw::default()),
            Event::TokenListing(TokenListing::default()),
            Event::OrderPlacement(OrderPlacement::default()),
            Event::OrderCancellation(OrderCancellation::default()),
            Event::OrderDeletion(OrderDeletion::default()),
            Event::Trade(Trade::default()),
            Event::TradeReversion(TradeReversion::default()),
            Event::SolutionSubmission(SolutionSubmission::default()),
        ];

        let events: BTreeMap<EventSortKey, Value> = event_list
            .iter()
            .enumerate()
            .map(|(i, event)| {
                (
                    EventSortKey {
                        block_number: i as u64,
                        block_hash: H256::zero(),
                        log_index: 1,
                    },
                    Value {
                        event: event.clone(),
                        batch_id: 0,
                    },
                )
            })
            .collect();
        let orderbook = Orderbook { events };
        let serialized_orderbook =
            bincode::serialize(&orderbook).expect("Failed to serialize orderbook");
        let deserialized_orderbook = Orderbook::try_from(&serialized_orderbook[..])
            .expect("Failed to deserialize orderbook");
        assert_eq!(orderbook.events, deserialized_orderbook.events);
    }

    #[test]
    #[ignore]
    fn test_write_read_recover_full_cycle() {
        let event_key = EventSortKey {
            block_number: 0,
            block_hash: H256::zero(),
            log_index: 1,
        };
        let event = Event::Deposit(Deposit {
            user: Address::from_low_u64_be(1),
            token: Address::from_low_u64_be(2),
            amount: 1.into(),
            batch_id: 2,
        });
        let value = Value { event, batch_id: 0 };

        let mut events = BTreeMap::new();
        events.insert(event_key, value);
        let initial_orderbook = Orderbook { events };

        let test_path = Path::new("/tmp/my_test_orderbook.ron");
        initial_orderbook.write_to_file(test_path).unwrap();

        let recovered_orderbook = Orderbook::try_from(test_path).unwrap();
        assert_eq!(initial_orderbook.events, recovered_orderbook.events);

        // Cleanup the file created here.
        assert!(fs::remove_file(test_path).is_ok());
    }

    #[test]
    fn delete_events_starting_at_block() {
        let mut orderbook = Orderbook::default();
        let token_listing_0 = EventData::Added(Event::TokenListing(TokenListing {
            token: Address::from_low_u64_be(0),
            id: 0,
        }));
        orderbook.handle_event_data(token_listing_0, 0, 0, H256::zero(), 0);
        let token_listing_1 = EventData::Added(Event::TokenListing(TokenListing {
            token: Address::from_low_u64_be(1),
            id: 1,
        }));
        orderbook.handle_event_data(token_listing_1, 0, 1, H256::zero(), 0);
        let order_placement = EventData::Added(Event::OrderPlacement(OrderPlacement {
            owner: Address::from_low_u64_be(2),
            index: 0,
            buy_token: 1,
            sell_token: 0,
            valid_from: 0,
            valid_until: 10,
            price_numerator: 10,
            price_denominator: 10,
        }));
        orderbook.handle_event_data(order_placement, 0, 2, H256::zero(), 0);

        let deposit_0 = EventData::Added(Event::Deposit(Deposit {
            user: Address::from_low_u64_be(2),
            token: Address::from_low_u64_be(0),
            amount: 1.into(),
            batch_id: 0,
        }));
        orderbook.handle_event_data(deposit_0, 0, 3, H256::zero(), 0);
        let deposit_1 = EventData::Added(Event::Deposit(Deposit {
            user: Address::from_low_u64_be(2),
            token: Address::from_low_u64_be(0),
            amount: 1.into(),
            batch_id: 0,
        }));
        orderbook.handle_event_data(deposit_1, 1, 0, H256::zero(), 0);
        let deposit_2 = EventData::Added(Event::Deposit(Deposit {
            user: Address::from_low_u64_be(2),
            token: Address::from_low_u64_be(0),
            amount: 1.into(),
            batch_id: 0,
        }));
        orderbook.handle_event_data(deposit_2, 2, 0, H256::zero(), 0);

        let auction_data = orderbook.get_auction_data(1.into()).unwrap();
        assert_eq!(
            auction_data.0.read_balance(0, Address::from_low_u64_be(2)),
            3
        );
        orderbook.delete_events_starting_at_block(1);
        let auction_data = orderbook.get_auction_data(1.into()).unwrap();
        assert_eq!(
            auction_data.0.read_balance(0, Address::from_low_u64_be(2)),
            1
        );
    }

    #[test]
    fn events_get_sorted() {
        // We add a token, deposit that token, request to withdraw that token, withdraw it but give
        // the events to the orderbook in a shuffled order.
        let token_listing_0 = EventData::Added(Event::TokenListing(TokenListing {
            token: Address::from_low_u64_be(0),
            id: 0,
        }));
        let token_listing_1 = EventData::Added(Event::TokenListing(TokenListing {
            token: Address::from_low_u64_be(1),
            id: 1,
        }));
        let order_placement = EventData::Added(Event::OrderPlacement(OrderPlacement {
            owner: Address::from_low_u64_be(2),
            index: 0,
            buy_token: 1,
            sell_token: 0,
            valid_from: 0,
            valid_until: 10,
            price_numerator: 10,
            price_denominator: 10,
        }));
        let deposit = EventData::Added(Event::Deposit(Deposit {
            user: Address::from_low_u64_be(2),
            token: Address::from_low_u64_be(0),
            amount: 10.into(),
            batch_id: 0,
        }));
        let withdraw_request = EventData::Added(Event::WithdrawRequest(WithdrawRequest {
            user: Address::from_low_u64_be(2),
            token: Address::from_low_u64_be(0),
            amount: 5.into(),
            batch_id: 0,
        }));
        let withdraw = EventData::Added(Event::Withdraw(Withdraw {
            user: Address::from_low_u64_be(2),
            token: Address::from_low_u64_be(0),
            amount: 3.into(),
        }));

        let mut orderbook = Orderbook::default();
        orderbook.handle_event_data(token_listing_1, 0, 1, H256::zero(), 0);
        orderbook.handle_event_data(order_placement, 0, 2, H256::zero(), 0);
        orderbook.handle_event_data(withdraw, 2, 0, H256::zero(), 300);
        orderbook.handle_event_data(deposit, 0, 3, H256::zero(), 0);
        orderbook.handle_event_data(withdraw_request, 1, 0, H256::zero(), 0);
        orderbook.handle_event_data(token_listing_0, 0, 0, H256::zero(), 0);

        let auction_data = orderbook.get_auction_data(2.into()).unwrap();
        assert_eq!(
            auction_data.0.read_balance(0, Address::from_low_u64_be(2)),
            7
        );
    }
}
