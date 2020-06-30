//! This module contains an implementation for querying historic echange data by
//! inspecting indexed events.

pub mod events;

use self::events::EventRegistry;
use anyhow::Result;
use std::{fs::File, io::Read, path::Path};

/// Historic exchange data.
pub struct ExchangeHistory {
    _events: EventRegistry,
}

impl ExchangeHistory {
    /// Reads historic exchange events from an `EventRegistry` bincode
    /// representation.
    pub fn read(read: impl Read) -> Result<Self> {
        let events = EventRegistry::read(read)?;

        Ok(ExchangeHistory { _events: events })
    }

    /// Reads historic exchange events from an `EventRegistry` filestore.
    pub fn from_filestore(filestore: impl AsRef<Path>) -> Result<Self> {
        ExchangeHistory::read(File::open(filestore)?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{contracts::stablex_contract::batch_exchange, models::BatchId};
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

    fn token_listing(token: u16) -> batch_exchange::Event {
        batch_exchange::Event::TokenListing(batch_exchange::event_data::TokenListing {
            id: token,
            token: Address::from_low_u64_be(token as _),
        })
    }

    #[test]
    fn read_event_filestore() {
        let bincode = {
            let mut events = EventRegistry::default();
            events.handle_event_data(
                EventData::Added(token_listing(0)),
                1,
                0,
                block_hash(1),
                batch_timestamp(41),
            );
            events.handle_event_data(
                EventData::Added(token_listing(1)),
                2,
                0,
                block_hash(2),
                batch_timestamp(41),
            );
            events.handle_event_data(
                EventData::Added(token_listing(2)),
                2,
                1,
                block_hash(2),
                batch_timestamp(41),
            );
            events.handle_event_data(
                EventData::Added(token_listing(3)),
                4,
                0,
                block_hash(4),
                batch_timestamp(42),
            );
            events.to_bytes().unwrap()
        };

        let history = ExchangeHistory::read(&*bincode).unwrap();
        assert_eq!(
            history._events.into_events().collect::<Vec<_>>(),
            vec![
                (token_listing(0), BatchId(41)),
                (token_listing(1), BatchId(41)),
                (token_listing(2), BatchId(41)),
                (token_listing(3), BatchId(42)),
            ],
        );
    }
}
