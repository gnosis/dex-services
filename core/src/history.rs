//! This module contains an implementation for quering historic echange data by
//! inspecting indexed events.

use crate::{
    contracts::stablex_contract::{batch_exchange, StableXContract},
    models::BatchId,
    orderbook::streamed::{orderbook, BlockTimestampBatchReading},
};
use anyhow::{anyhow, bail, Result};
use ethcontract::{BlockNumber, EventData};
use std::{collections::HashMap, convert::TryFrom, path::Path};

/// Historic exchange data.
pub struct ExchangeHistory {
    _events: Vec<(batch_exchange::Event, BatchId)>,
}

impl ExchangeHistory {
    /// Initializes a new indexed orderbook from a web3 provider.
    pub async fn initialize(
        contract: &impl StableXContract,
        block_timestamp_reader: &impl BlockTimestampBatchReading,
        orderbook_filestore: Option<&Path>,
    ) -> Result<Self> {
        let (starting_block, mut events) = orderbook_filestore
            .and_then(events_from_filestore)
            .unwrap_or_else(|| (BlockNumber::Earliest, Vec::new()));

        let queried_events = contract
            .past_events(starting_block, BlockNumber::Latest)
            .await?
            .into_iter()
            .map(|event| {
                let event_data = match event.data {
                    EventData::Added(data) => data,
                    EventData::Removed(data) => {
                        bail!("past removed event {:?} ({:?})", data, event.meta);
                    }
                };
                let metadata = event
                    .meta
                    .ok_or_else(|| anyhow!("past event missing metadata {:?}", event_data))?;

                Ok((event_data, metadata.block_hash))
            })
            .collect::<Result<Vec<_>>>()?;
        let block_timestamps = block_timestamp_reader
            .block_timestamps(
                queried_events
                    .iter()
                    .map(|(_, block_hash)| *block_hash)
                    .collect(),
            )
            .await?
            .into_iter()
            .collect::<HashMap<_, _>>();

        events.reserve(queried_events.len());
        for (event, block_hash) in queried_events {
            let block_timestamp = block_timestamps.get(&block_hash).ok_or_else(|| {
                anyhow!(
                    "missing block timestamp for event {:?} on block {:?}",
                    event,
                    block_hash
                )
            })?;
            let batch_id = BatchId::from_timestamp(*block_timestamp);

            events.push((event, batch_id));
        }

        Ok(ExchangeHistory { _events: events })
    }
}

/// Extracts exchange events
fn events_from_filestore(
    filestore: impl AsRef<Path>,
) -> Option<(BlockNumber, Vec<(batch_exchange::Event, BatchId)>)> {
    let orderbook = match orderbook::Orderbook::try_from(filestore.as_ref()) {
        Ok(orderbook) => orderbook,
        Err(err) => {
            log::warn!("Error reading orderbook filestore: {}", err);
            return None;
        }
    };

    let last_block = BlockNumber::Number(orderbook.last_handled_block()?.into());
    let events = orderbook
        .into_events()
        .map(|(event, batch_id)| (event, BatchId(batch_id as _)))
        .collect();

    Some((last_block, events))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        contracts::stablex_contract::MockStableXContract,
        orderbook::streamed::block_timestamp_reading::MockBlockTimestampBatchReading,
    };
    use ethcontract::{Address, Event, EventMetadata, H256};
    use futures::future::FutureExt;
    use mockall::predicate::eq;

    fn block_hash(block_number: u64) -> H256 {
        H256::from_low_u64_be(block_number)
    }

    fn metadata(block_number: u64) -> EventMetadata {
        EventMetadata {
            block_hash: block_hash(block_number),
            block_number,

            // NOTE: The following event metadata members currently aren't used.
            transaction_hash: H256::zero(),
            transaction_index: 0,
            log_index: 0,
            transaction_log_index: None,
            log_type: None,
        }
    }

    fn token_listing(token: u16) -> batch_exchange::Event {
        batch_exchange::Event::TokenListing(batch_exchange::event_data::TokenListing {
            id: token,
            token: Address::from_low_u64_be(token as _),
        })
    }

    #[test]
    fn matches_events_to_timestamps() {
        let mut contract = MockStableXContract::new();
        contract
            .expect_past_events()
            .with(eq(BlockNumber::Earliest), eq(BlockNumber::Latest))
            .returning(|_, _| {
                async {
                    Ok(vec![
                        Event {
                            data: EventData::Added(token_listing(0)),
                            meta: Some(metadata(1)),
                        },
                        Event {
                            data: EventData::Added(token_listing(1)),
                            meta: Some(metadata(2)),
                        },
                        Event {
                            data: EventData::Added(token_listing(2)),
                            meta: Some(metadata(2)),
                        },
                        Event {
                            data: EventData::Added(token_listing(3)),
                            meta: Some(metadata(4)),
                        },
                    ])
                }
                .boxed()
            });

        let mut block_timestamps = MockBlockTimestampBatchReading::new();
        block_timestamps
            .expect_block_timestamps()
            .with(eq(
                hash_set! { block_hash(1), block_hash(2), block_hash(4) },
            ))
            .returning(|_| {
                async {
                    Ok(vec![
                        (block_hash(1), 41 * 300),
                        (block_hash(2), 41 * 300),
                        (block_hash(4), 42 * 300),
                    ])
                }
                .boxed()
            });

        let history = ExchangeHistory::initialize(&contract, &block_timestamps, None)
            .now_or_never()
            .unwrap()
            .unwrap();

        assert_eq!(
            history._events,
            vec![
                (token_listing(0), BatchId(41)),
                (token_listing(1), BatchId(41)),
                (token_listing(2), BatchId(41)),
                (token_listing(3), BatchId(42)),
            ],
        );
    }
}
