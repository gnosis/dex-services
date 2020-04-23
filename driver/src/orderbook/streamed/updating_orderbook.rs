use super::*;
use crate::contracts::stablex_contract::batch_exchange;
use anyhow::{anyhow, bail, Result};
use block_timestamp::BlockTimestamp;
use ethcontract::{contract::Event, errors::ExecutionError};
use futures::{
    channel::oneshot,
    future::{BoxFuture, FutureExt},
    select,
    stream::{BoxStream, StreamExt as _},
};
use orderbook::Orderbook;
use std::sync::{Arc, Mutex};

// TODO:
// `pub struct UpdatingOrderbook` that also owns an `Arc<Mutex<Orderbook>>` and creates a
// background thread running `UpdatingOrderbookThread::update_with_events`.
// This way `UpdatingOrderbookThread` is independently testable.

/// Update the orderbook with events from the stream forever or until exit_indicator is dropped.
///
/// Returns Ok when exit_indicator is dropped.
/// Returns Err if the stream ends.
async fn update_with_events_forever(
    orderbook: Arc<Mutex<Orderbook>>,
    mut block_timestamp: impl BlockTimestamp,
    exit_indicator: oneshot::Receiver<()>,
    past_events: BoxFuture<'_, Result<Vec<Event<batch_exchange::Event>>, ExecutionError>>,
    stream: BoxStream<'_, Result<Event<batch_exchange::Event>, ExecutionError>>,
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
        handle_event(&orderbook, &mut block_timestamp, event).await?;
    }

    loop {
        let event = select! {
                event = stream.next() =>
                    event.ok_or(anyhow!("stream ended"))??,
                _ = exit_indicator => return Ok(()),
        };
        handle_event(&orderbook, &mut block_timestamp, event).await?;
    }
}

/// Apply a single event to the orderbook.
async fn handle_event(
    orderbook: &Mutex<Orderbook>,
    block_timestamp: &mut impl BlockTimestamp,
    event: Event<batch_exchange::Event>,
) -> Result<()> {
    match event {
        Event {
            data,
            meta: Some(meta),
        } => {
            let block_timestamp = block_timestamp.block_timestamp(meta.block_hash).await?;
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
