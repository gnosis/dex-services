use super::*;
use crate::{
    contracts::{stablex_contract::StableXContract, Web3},
    history::events::EventRegistry,
    models::{AccountState, BatchId, Order},
    orderbook::StableXOrderBookReading,
};
use anyhow::{anyhow, bail, ensure, Result};
use block_timestamp_reading::{BlockTimestampReading, CachedBlockTimestampReader};
use ethcontract::{contract::Event, BlockNumber, H256};
use futures::future::{BoxFuture, FutureExt as _};
use futures::lock::Mutex;
use log::{error, info, warn};
use std::collections::HashSet;
use std::convert::TryFrom;
use std::path::PathBuf;
use std::sync::Arc;

const BLOCK_CONFIRMATION_COUNT: u64 = 25;

/// An event based orderbook that automatically updates itself with new events from the contract.
pub struct UpdatingOrderbook {
    contract: Arc<dyn StableXContract>,
    web3: Web3,
    block_page_size: usize,
    /// We need a mutex because otherwise the struct wouldn't be Sync which is needed because we use
    /// the orderbook in multiple threads. The mutex is locked in `get_auction_data_for_batch` while
    /// the orderbook is updated with new events.
    /// None means that we have not yet been initialized.
    context: Mutex<Option<Context>>,
    /// File path where orderbook is written to disk.
    filestore: Option<PathBuf>,
}

struct Context {
    orderbook: EventRegistry,
    last_handled_block: u64,
    block_timestamp_reader: CachedBlockTimestampReader<Web3>,
}

impl UpdatingOrderbook {
    /// Does not block on initializing the orderbook. This will happen in the first call to
    /// `get_auction_data_*` which can thus take a long time to complete.
    pub fn new(
        contract: Arc<dyn StableXContract>,
        web3: Web3,
        block_page_size: usize,
        path: Option<PathBuf>,
    ) -> Self {
        Self {
            contract,
            web3,
            block_page_size,
            context: Mutex::new(None),
            filestore: path,
        }
    }

    /// Recover the orderbook from file if possible.
    fn load_orderbook_from_file(&self, context: &mut Context) {
        // TODO: use async file io
        match &self.filestore {
            Some(path) => match EventRegistry::try_from(path.as_path()) {
                Ok(orderbook) => {
                    info!("successfully recovered orderbook from path");
                    context.last_handled_block = orderbook.last_handled_block().unwrap_or(0);
                    context.orderbook = orderbook;
                }
                Err(error) => {
                    if path.exists() {
                        // Exclude warning when file doesn't exist (i.e. on first startup)
                        warn!(
                            "Failed to construct orderbook from path (using default): {:?}",
                            error,
                        );
                    } else {
                        info!(
                            "Orderbook at specified path not found: {:?}",
                            error,
                        );
                    }
                }
            },
            None => (),
        };
    }

    /// Use the context, ensuring that the orderbook has been initialized and updated.
    async fn do_with_context<T, F>(&self, callback: F) -> Result<T>
    where
        F: for<'ctx> FnOnce(&'ctx mut Context) -> BoxFuture<'ctx, Result<T>>,
    {
        let mut context_guard = self.context.lock().await;
        match context_guard.as_mut() {
            Some(context) => {
                self.update_with_events(context).await?;
                callback(context).await
            }
            None => {
                let mut context = Context {
                    orderbook: EventRegistry::default(),
                    last_handled_block: 0,
                    block_timestamp_reader: CachedBlockTimestampReader::new(
                        self.web3.clone(),
                        BLOCK_CONFIRMATION_COUNT,
                    ),
                };
                self.load_orderbook_from_file(&mut context);
                self.update_with_events(&mut context).await?;
                let result = callback(&mut context).await;
                *context_guard = Some(context);
                result
            }
        }
    }

    async fn update_with_events(&self, context: &mut Context) -> Result<()> {
        let current_block = self.web3.eth().block_number().await?.as_u64();
        let from_block = context
            .last_handled_block
            .saturating_sub(BLOCK_CONFIRMATION_COUNT);
        ensure!(
            from_block <= current_block,
            format!(
                "current block number according to node is {} which is more than {} blocks in the \
                 past compared to previous current block {}",
                current_block, BLOCK_CONFIRMATION_COUNT, from_block
            )
        );
        // We cannot use BlockNumber::Pending here because we are not guaranteed to get metadata for
        // pending blocks but we need the metadata in the functions below.
        let to_block = BlockNumber::Number(current_block.into());
        log::info!(
            "Updating event based orderbook from block {} to block {}.",
            from_block,
            current_block,
        );
        let events = self
            .contract
            .past_events(
                BlockNumber::Number(from_block.into()),
                to_block,
                self.block_page_size as _,
            )
            .await?;
        self.handle_events(context, events, from_block, current_block)
            .await?;
        // Update the orderbook on disk before exit.
        if let Some(filestore) = &self.filestore {
            if let Err(write_error) = context.orderbook.write_to_file(filestore) {
                error!("Failed to write to orderbook {}", write_error);
            }
        }
        context.last_handled_block = current_block;
        Ok(())
    }

    async fn handle_events(
        &self,
        context: &mut Context,
        events: Vec<Event<contracts::batch_exchange::Event>>,
        delete_events_starting_at_block: u64,
        latest_block: u64,
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
            .prepare_cache(block_hashes, self.block_page_size, latest_block)
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
        event: Event<contracts::batch_exchange::Event>,
    ) -> Result<()> {
        match event {
            Event {
                data,
                meta: Some(meta),
            } => {
                let block_timestamp = context
                    .block_timestamp_reader
                    .block_timestamp(meta.block_hash.into())
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

#[async_trait::async_trait]
impl StableXOrderBookReading for UpdatingOrderbook {
    /// Blocks on updating the orderbook. This can be expensive if `initialize` hasn't been called before.
    async fn get_auction_data_for_batch(
        &self,
        batch_id_to_solve: u32,
    ) -> Result<(AccountState, Vec<Order>)> {
        self.do_with_context(move |context| {
            immediate!(context.orderbook.auction_state_for_batch(batch_id_to_solve))
        })
        .await
    }

    async fn get_auction_data_for_block(
        &self,
        block: BlockNumber,
    ) -> Result<(AccountState, Vec<Order>)> {
        self.do_with_context(move |context| {
            async move {
                // NOTE: Get the exact timestamp for the block, this will give more
                // accurate results as to what batch the orderbook is retrieved for.
                let timestamp = context
                    .block_timestamp_reader
                    .block_timestamp(block.into())
                    .await?;
                let batch = BatchId::from_timestamp(timestamp);

                context
                    .orderbook
                    .auction_state_for_batch_at_block(batch, block)
            }
            .boxed()
        })
        .await
    }

    async fn initialize(&self) -> Result<()> {
        self.do_with_context(|_| immediate!(Ok(()))).await
    }
}
