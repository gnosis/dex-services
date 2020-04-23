use super::*;
use crate::{
    contracts::stablex_contract::{batch_exchange, StableXContract},
    models::{AccountState, Order},
    orderbook::StableXOrderBookReading,
};
use anyhow::{anyhow, bail, Result};
use block_timestamp_reading::{BlockTimestampReading, MemoizingBlockTimestampReader};
use ethcontract::{contract::Event, errors::ExecutionError};
use futures::{
    channel::oneshot,
    future::{BoxFuture, FutureExt},
    select,
    stream::{BoxStream, StreamExt as _},
};
use orderbook::Orderbook;
use std::sync::{Arc, Mutex};

/// An event based orderbook that automatically updates itself with new events from the contract.
#[derive(Debug)]
pub struct UpdatingOrderbook {
    orderbook: Arc<Mutex<Orderbook>>,
    // When this struct is dropped this sender will be dropped which makes the updater thread stop.
    _channel: oneshot::Sender<()>,
}

impl UpdatingOrderbook {
    pub fn new(
        _contract: &impl StableXContract,
        block_timestamp_reader: impl BlockTimestampReading + Send + 'static,
    ) -> Self {
        let orderbook = Arc::new(Mutex::new(Orderbook::default()));
        let orderbook_clone = orderbook.clone();
        let (sender, receiver) = oneshot::channel();
        // Create stream first to make sure we do not miss any events between it and past events.
        // TODO: use the real functions once they are implemented
        let stream = futures::stream::iter(vec![]).boxed(); // contract.stream_events();
        let past_events = futures::future::ready(Ok(Vec::new())).boxed(); // contract.past_events();

        std::thread::spawn(move || {
            futures::executor::block_on(update_with_events_forever(
                orderbook_clone,
                MemoizingBlockTimestampReader::new(block_timestamp_reader),
                receiver,
                past_events,
                stream,
            ))
        });

        Self {
            orderbook,
            _channel: sender,
        }
    }
}

impl StableXOrderBookReading for UpdatingOrderbook {
    fn get_auction_data(&self, index: U256) -> Result<(AccountState, Vec<Order>)> {
        self.orderbook
            .lock()
            .map_err(|err| anyhow!("poison error: {}", err))?
            .get_auction_data(index)
    }
}

/// Update the orderbook with events from the stream forever or until exit_indicator is dropped.
///
/// Returns Ok when exit_indicator is dropped.
/// Returns Err if the stream ends.
async fn update_with_events_forever(
    orderbook: Arc<Mutex<Orderbook>>,
    mut block_timestamp_reader: impl BlockTimestampReading,
    exit_indicator: oneshot::Receiver<()>,
    past_events: BoxFuture<'static, Result<Vec<Event<batch_exchange::Event>>, ExecutionError>>,
    stream: BoxStream<'static, Result<Event<batch_exchange::Event>, ExecutionError>>,
) -> Result<()> {
    // `select!` requires the futures to be fused.
    // By selecting over exit_indicator and the future/stream we make sure to exit immediately when
    // exit_indicator is dropped without waiting for the future or the stream to produce a value.
    let mut exit_indicator = exit_indicator.fuse();
    let mut past_events = past_events.fuse();
    let mut stream = stream.fuse();

    let past_events = select! {
        past_events = past_events => past_events?,
        _ = exit_indicator => return Ok(()),
    };
    for event in past_events {
        handle_event(&orderbook, &mut block_timestamp_reader, event).await?;
    }

    loop {
        let event = select! {
                event = stream.next() =>
                    event.ok_or(anyhow!("stream ended"))??,
                _ = exit_indicator => return Ok(()),
        };
        handle_event(&orderbook, &mut block_timestamp_reader, event).await?;
    }
}

/// Apply a single event to the orderbook.
async fn handle_event(
    orderbook: &Mutex<Orderbook>,
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
            orderbook
                .lock()
                .map_err(|e| anyhow!("poison error: {}", e))?
                .handle_event_data(
                    data,
                    meta.block_number,
                    meta.log_index,
                    meta.block_hash,
                    block_timestamp,
                );
            Ok(())
        }
        Event { meta: None, .. } => bail!("event without metadata"),
    }
}
