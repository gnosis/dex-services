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
        let file_content = ron::ser::to_string_pretty(self, ron::ser::PrettyConfig::default())?;
        temp_file.write_all(file_content.as_bytes())?;

        // Rename the temp file to the originally specified path.
        fs::rename(temp_path, path)?;
        Ok(())
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
        ron::de::from_bytes(bytes).context("Failed to load Orderbook")
    }
}

impl TryFrom<File> for Orderbook {
    type Error = anyhow::Error;

    fn try_from(mut file: File) -> Result<Self> {
        let mut contents = String::new();
        let bytes_read = file
            .read_to_string(&mut contents)
            .with_context(|| format!("Failed to read file: {:?}", file))?;
        info!(
            "Successfully loaded {} bytes from Orderbook file",
            bytes_read
        );
        Orderbook::try_from(contents.as_bytes())
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
        let orderbook = Orderbook { events };

        let serialized_orderbook =
            ron::ser::to_string_pretty(&orderbook, ron::ser::PrettyConfig::default()).unwrap();
        let deserialized_orderbook = Orderbook::try_from(serialized_orderbook.as_bytes()).unwrap();
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
        for block_number in 0..3 {
            orderbook.handle_event_data(
                EventData::Added(Event::Deposit(Deposit::default())),
                block_number,
                0,
                H256::zero(),
                0,
            );
        }
        assert_eq!(orderbook.events.len(), 3);
        orderbook.delete_events_starting_at_block(1);
        assert_eq!(orderbook.events.len(), 1);
        assert_eq!(orderbook.events.iter().next().unwrap().0.block_number, 0);
    }

    #[test]
    fn events_get_sorted() {
        let mut orderbook = Orderbook::default();
        // We are going to keep track of the correct order using the deposit's batch id.
        let mut add_event = |deposit_id, block_number, log_index| {
            orderbook.handle_event_data(
                EventData::Added(Event::Deposit(Deposit {
                    batch_id: deposit_id,
                    ..Default::default()
                })),
                block_number,
                log_index,
                H256::zero(),
                0,
            );
        };
        // Add a couple of events in random order.
        add_event(2, 0, 2);
        add_event(4, 1, 1);
        add_event(0, 0, 0);
        add_event(5, 1, 2);
        add_event(3, 1, 0);
        add_event(1, 0, 1);
        assert_eq!(orderbook.events.len(), 6);
        // Check that the order is correct.
        for (i, (_key, value)) in orderbook.events.iter().enumerate() {
            match &value.event {
                Event::Deposit(deposit) => assert_eq!(deposit.batch_id, i as u32),
                _ => unreachable!(),
            }
        }
    }
}
