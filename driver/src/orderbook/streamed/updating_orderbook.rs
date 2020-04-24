use super::*;
use crate::{
    contracts::{
        stablex_contract::{batch_exchange, StableXContract},
        Web3,
    },
    models::{AccountState, Order},
    orderbook::StableXOrderBookReading,
};
use anyhow::{anyhow, bail, ensure, Result};
use block_timestamp_reading::{BlockTimestampReading, CachedBlockTimestampReader};
use ethcontract::{contract::Event, BlockNumber, H256};
use futures::{
    channel::oneshot, compat::Future01CompatExt as _, future::FutureExt, pin_mut, select_biased,
};
use orderbook::Orderbook;
use std::collections::HashSet;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex,
};
use std::{process, thread, time::Duration};

/// An event based orderbook that automatically updates itself with new events from the contract.
#[derive(Debug)]
pub struct UpdatingOrderbook {
    orderbook: Arc<Mutex<Orderbook>>,
    // Indicates whether the background thread has caught up with past events at which point the
    // orderbook is ready to be read.
    orderbook_ready: Arc<AtomicBool>,
    // When this struct is dropped this sender will be dropped which makes the updater thread stop.
    _exit_tx: oneshot::Sender<()>,
}

impl UpdatingOrderbook {
    pub fn new(contract: Arc<dyn StableXContract + Send + Sync>, web3: Web3) -> Self {
        let orderbook = Arc::new(Mutex::new(Orderbook::default()));
        let orderbook_clone = orderbook.clone();
        let orderbook_ready = Arc::new(AtomicBool::new(false));
        let orderbook_ready_clone = orderbook_ready.clone();
        let (exit_tx, exit_rx) = oneshot::channel();

        std::thread::spawn(move || {
            let result = futures::executor::block_on(update_with_events_forever(
                web3.clone(),
                contract,
                orderbook_clone,
                orderbook_ready_clone,
                CachedBlockTimestampReader::new(web3),
                exit_rx,
            ));
            if let Err(err) = result {
                log::error!("event based orderbook failed: {:?}", err);
                // TODO: implement a retry mechanism
                // For now we error the program so force a restart of the whole driver because
                // without a retry we would be stuck with an outdated orderbook forever.
                // Sleep for one second, so that we have time to flush the logs.
                thread::sleep(Duration::from_secs(1));
                process::exit(1);
            }
        });

        Self {
            orderbook,
            orderbook_ready,
            _exit_tx: exit_tx,
        }
    }
}

impl StableXOrderBookReading for UpdatingOrderbook {
    fn get_auction_data(&self, batch_id_to_solve: U256) -> Result<(AccountState, Vec<Order>)> {
        ensure!(
            self.orderbook_ready.load(Ordering::SeqCst),
            "orderbook not yet ready"
        );
        self.orderbook
            .lock()
            .map_err(|err| anyhow!("poison error: {}", err))?
            .get_auction_data(batch_id_to_solve)
    }
}

/// Update the orderbook with events from the stream forever or until exit_indicator is dropped.
///
/// Returns Ok when exit_indicator is dropped.
/// Returns Err if the stream ends.
async fn update_with_events_forever(
    web3: Web3,
    contract: Arc<dyn StableXContract>,
    orderbook: Arc<Mutex<Orderbook>>,
    orderbook_ready: Arc<AtomicBool>,
    mut block_timestamp_reader: CachedBlockTimestampReader<Web3>,
    exit_indicator: oneshot::Receiver<()>,
) -> Result<()> {
    const POLL_INTERVALL: Duration = Duration::from_secs(15);
    const BLOCK_RANGE: u64 = 25;

    log::info!("Starting event based orderbook updating.");

    // `select!` requires the futures to be fused and pinned.
    let exit_indicator = exit_indicator.fuse();
    pin_mut!(exit_indicator);

    let mut last_handled_block = 0u64;
    loop {
        let current_block = web3.eth().block_number().compat().await?;
        let from_block = last_handled_block.saturating_sub(BLOCK_RANGE);
        let to_block = BlockNumber::Number(current_block);
        select_biased! {
            _ = exit_indicator => return Ok(()),
            events = contract.past_events(BlockNumber::Number(from_block.into()), to_block).fuse() => {
                handle_events(&orderbook, &mut block_timestamp_reader, events?, from_block).await?;
            },
        };
        last_handled_block = current_block.as_u64();
        // This will be set and stay true the first time we reach this point.
        orderbook_ready.store(true, Ordering::SeqCst);
        // TODO: This is not optimal because it means we might sleep for some time when exit
        // indicator triggered. We should create a sleep future that expires after poll interval and
        // join both together.
        std::thread::sleep(POLL_INTERVALL);
    }
}

/// Apply a vector of events to the orderbook.
async fn handle_events(
    orderbook: &Mutex<Orderbook>,
    block_timestamp_reader: &mut CachedBlockTimestampReader<Web3>,
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
    block_timestamp_reader.prepare_cache(block_hashes).await?;
    // Locking here ensures that the orderbook is not observable after the events have been deleted
    // but the new events not yet applied.
    let mut orderbook = orderbook
        .lock()
        .map_err(|err| anyhow!("poison error: {}", err))?;
    orderbook.delete_events_starting_at_block(delete_events_starting_at_block);
    for event in events {
        handle_event(&mut orderbook, block_timestamp_reader, event).await?;
    }
    log::info!("Finished applying events");
    Ok(())
}

/// Apply a single event to the orderbook.
async fn handle_event(
    orderbook: &mut Orderbook,
    block_timestamp_reader: &mut impl BlockTimestampReading,
    event: Event<batch_exchange::Event>,
) -> Result<()> {
    match event {
        Event {
            data,
            meta: Some(meta),
        } => {
            let block_timestamp = block_timestamp_reader
                .block_timestamp(meta.block_hash)
                .await?;
            orderbook.handle_event_data(
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
