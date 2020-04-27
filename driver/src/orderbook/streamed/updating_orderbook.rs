use super::*;
use crate::{
    contracts::{
        stablex_contract::{batch_exchange, StableXContract},
        Web3,
    },
    models::{AccountState, Order},
    orderbook::StableXOrderBookReading,
};
use anyhow::{anyhow, bail, Result};
use block_timestamp_reading::{BlockTimestampReading, CachedBlockTimestampReader};
use ethcontract::{contract::Event, BlockNumber, H256};
use futures::compat::Future01CompatExt as _;
use orderbook::Orderbook;
use std::collections::HashSet;
use std::sync::{Arc, Mutex};

/// An event based orderbook that automatically updates itself with new events from the contract.
pub struct UpdatingOrderbook {
    contract: Arc<dyn StableXContract + Send + Sync>,
    web3: Web3,
    /// We need a mutex because otherwise the struct wouldn't be Sync which is needed because we use
    /// the orderbook in multiple threads. The mutex is locked in `get_auction_data` while the
    /// orderbook is updated with new events.
    behind_mutex: Mutex<BehindMutex>,
}

struct BehindMutex {
    orderbook: Orderbook,
    last_handled_block: u64,
    block_timestamp_reader: CachedBlockTimestampReader<Web3>,
}

impl UpdatingOrderbook {
    /// Blocks on attempting to build the orderbook. After the initial build the orderbook is
    /// updated every time `get_auction_data` is called.
    pub fn new(contract: Arc<dyn StableXContract + Send + Sync>, web3: Web3) -> Result<Self> {
        let result = Self {
            contract,
            web3: web3.clone(),
            behind_mutex: Mutex::new(BehindMutex {
                orderbook: Orderbook::default(),
                last_handled_block: 0,
                block_timestamp_reader: CachedBlockTimestampReader::new(web3),
            }),
        };

        // Perform the initial update.
        {
            let mut behind_mutex = result
                .behind_mutex
                .lock()
                .map_err(|err| anyhow!("poison error: {}", err))?;
            futures::executor::block_on(result.update_with_events(&mut behind_mutex))?;
        }

        Ok(result)
    }

    async fn update_with_events(&self, behind_mutex: &mut BehindMutex) -> Result<()> {
        const BLOCK_RANGE: u64 = 25;
        log::info!("Starting event based orderbook updating.");
        let current_block = self.web3.eth().block_number().compat().await?;
        let from_block = behind_mutex.last_handled_block.saturating_sub(BLOCK_RANGE);
        let to_block = BlockNumber::Number(current_block);
        log::info!(
            "The range is from block {} to block {}",
            from_block,
            current_block.as_u64(),
        );
        let events = self
            .contract
            .past_events(BlockNumber::Number(from_block.into()), to_block)
            .await?;
        self.handle_events(behind_mutex, events, from_block).await?;
        behind_mutex.last_handled_block = current_block.as_u64();
        Ok(())
    }

    async fn handle_events(
        &self,
        behind_mutex: &mut BehindMutex,
        events: Vec<Event<batch_exchange::Event>>,
        delete_events_starting_at_block: u64,
    ) -> Result<()> {
        log::info!("Received {} events.", events.len());
        let block_hashes = events
            .iter()
            .map(|event| {
                let metadata = event
                    .meta
                    .as_ref()
                    .ok_or_else(|| anyhow!("event without metadata: {:?}", event))?;
                Ok(metadata.block_hash)
            })
            .collect::<Result<HashSet<H256>>>()?;
        behind_mutex
            .block_timestamp_reader
            .prepare_cache(block_hashes)
            .await?;
        behind_mutex
            .orderbook
            .delete_events_starting_at_block(delete_events_starting_at_block);
        for event in events {
            self.handle_event(behind_mutex, event).await?;
        }
        log::info!("Finished applying events");
        Ok(())
    }

    /// Apply a single event to the orderbook.
    async fn handle_event(
        &self,
        behind_mutex: &mut BehindMutex,
        event: Event<batch_exchange::Event>,
    ) -> Result<()> {
        match event {
            Event {
                data,
                meta: Some(meta),
            } => {
                let block_timestamp = behind_mutex
                    .block_timestamp_reader
                    .block_timestamp(meta.block_hash)
                    .await?;
                behind_mutex.orderbook.handle_event_data(
                    data,
                    meta.block_number,
                    meta.log_index,
                    meta.block_hash,
                    block_timestamp,
                );
            }
            Event { meta: None, .. } => bail!("event without metadata"),
        }
        Ok(())
    }
}

impl StableXOrderBookReading for UpdatingOrderbook {
    /// Blocks on updating the orderbook.
    fn get_auction_data(&self, batch_id_to_solve: U256) -> Result<(AccountState, Vec<Order>)> {
        let mut behind_mutex = self
            .behind_mutex
            .lock()
            .map_err(|err| anyhow!("poison error: {}", err))?;
        futures::executor::block_on(self.update_with_events(&mut behind_mutex))?;
        behind_mutex.orderbook.get_auction_data(batch_id_to_solve)
    }
}
