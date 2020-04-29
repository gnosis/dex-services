use super::*;
use crate::{
    contracts::{
        stablex_contract::{batch_exchange, StableXContract},
        Web3,
    },
    models::{AccountState, Order},
    orderbook::StableXOrderBookReading,
    util::FutureWaitExt as _,
};
use anyhow::{anyhow, bail, Result};
use block_timestamp_reading::{BlockTimestampReading, CachedBlockTimestampReader};
use ethcontract::{contract::Event, BlockNumber, H256};
use futures::compat::Future01CompatExt as _;
use log::{error, info, warn};
use orderbook::Orderbook;
use std::collections::HashSet;
use std::convert::TryFrom;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

/// An event based orderbook that automatically updates itself with new events from the contract.
pub struct UpdatingOrderbook {
    contract: Arc<dyn StableXContract + Send + Sync>,
    web3: Web3,
    /// We need a mutex because otherwise the struct wouldn't be Sync which is needed because we use
    /// the orderbook in multiple threads. The mutex is locked in `get_auction_data` while the
    /// orderbook is updated with new events.
    context: Mutex<Context>,
    /// File path where orderbook is written to disk.
    filestore: Option<PathBuf>,
}

struct Context {
    orderbook: Orderbook,
    last_handled_block: u64,
    block_timestamp_reader: CachedBlockTimestampReader<Web3>,
}

impl UpdatingOrderbook {
    /// Does not block on initializing the orderbook. This will happen in the first call to
    /// `get_auction_data` which can thus take a long time to complete.
    pub fn new(
        contract: Arc<dyn StableXContract + Send + Sync>,
        web3: Web3,
        path: Option<PathBuf>,
    ) -> Self {
        // Recover the orderbook from file (if possible, otherwise use default).
        let orderbook = match &path {
            Some(path) => match Orderbook::try_from(path.as_path()) {
                Ok(orderbook) => {
                    info!("successfully recovered orderbook from path");
                    orderbook
                }
                Err(error) => {
                    // This will always happen when file does not exist (i.e. on first startup)
                    // TODO: match on this error and don't warn when file doesn't exist.
                    warn!(
                        "Failed to construct orderbook from path (using default): {}",
                        error
                    );
                    Orderbook::default()
                }
            },
            None => Orderbook::default(),
        };
        let last_handled_block = orderbook.last_handled_block().unwrap_or(0);
        Self {
            contract,
            web3: web3.clone(),
            context: Mutex::new(Context {
                orderbook,
                last_handled_block,
                block_timestamp_reader: CachedBlockTimestampReader::new(web3),
            }),
            filestore: path,
        }
    }

    async fn update_with_events(&self, context: &mut Context) -> Result<()> {
        const BLOCK_RANGE: u64 = 25;
        let current_block = self.web3.eth().block_number().compat().await?;
        let from_block = context.last_handled_block.saturating_sub(BLOCK_RANGE);
        // We cannot use BlockNumber::Pending here because we are not guaranteed to get metadata for
        // pending blocks but we need the metadata in the functions below.
        let to_block = BlockNumber::Number(current_block);
        log::info!(
            "Updating event based orderbook with from block {} to block {}.",
            from_block,
            current_block.as_u64(),
        );
        let events = self
            .contract
            .past_events(BlockNumber::Number(from_block.into()), to_block)
            .await?;
        self.handle_events(context, events, from_block).await?;
        // Update the orderbook on disk before exit.
        if let Some(filestore) = &self.filestore {
            if let Err(write_error) = context.orderbook.write_to_file(filestore) {
                error!("Failed to write to orderbook {}", write_error);
            }
        }
        context.last_handled_block = current_block.as_u64();
        Ok(())
    }

    async fn handle_events(
        &self,
        context: &mut Context,
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
        context
            .block_timestamp_reader
            .prepare_cache(block_hashes)
            .await?;
        context
            .orderbook
            .delete_events_starting_at_block(delete_events_starting_at_block);
        for event in events {
            self.handle_event(context, event).await?;
        }
        log::info!("Finished applying events");
        Ok(())
    }

    /// Apply a single event to the orderbook.
    async fn handle_event(
        &self,
        context: &mut Context,
        event: Event<batch_exchange::Event>,
    ) -> Result<()> {
        match event {
            Event {
                data,
                meta: Some(meta),
            } => {
                let block_timestamp = context
                    .block_timestamp_reader
                    .block_timestamp(meta.block_hash)
                    .await?;
                context.orderbook.handle_event_data(
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
    /// Blocks on updating the orderbook. When this is called the first time this can take several
    /// minutes.
    fn get_auction_data(&self, batch_id_to_solve: U256) -> Result<(AccountState, Vec<Order>)> {
        let mut context = self
            .context
            .lock()
            .map_err(|err| anyhow!("poison error: {}", err))?;
        self.update_with_events(&mut context).wait()?;
        context.orderbook.get_auction_data(batch_id_to_solve)
    }
}
