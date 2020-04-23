use super::*;
use crate::contracts::stablex_contract::batch_exchange;
use anyhow::{anyhow, bail, Result};
use block_timestamp::BlockTimestamp;
use ethcontract::{contract::Event, errors::ExecutionError};
use futures::{
    channel::oneshot,
    future::FutureExt,
    pin_mut, select_biased,
    stream::{Stream, StreamExt as _},
};
use orderbook::Orderbook;
use std::future::Future;
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
    past_events: impl Future<Output = Result<Vec<Event<batch_exchange::Event>>, ExecutionError>>,
    stream: impl Stream<Item = Result<Event<batch_exchange::Event>, ExecutionError>>,
) -> Result<()> {
    // `select!` requires the futures to be fused...
    let exit_indicator = exit_indicator.fuse();
    let past_events = past_events.fuse();
    let stream = stream.fuse();
    // ...and pinned.
    pin_mut!(exit_indicator);
    pin_mut!(past_events);
    pin_mut!(stream);

    loop {
        // We select over everything together instead of for example the past events first then the
        // stream to ensure that the stream gets polled at least once which it needs in order to
        // create the corresponding filter on the node.
        select_biased! {
            _ = exit_indicator => return Ok(()),
            event = stream.next() => {
                let event = event.ok_or(anyhow!("stream ended"))??;
                handle_event(&orderbook, &mut block_timestamp, event).await?;
            },
            past_events = past_events => {
                for event in past_events? {
                    handle_event(&orderbook, &mut block_timestamp, event).await?;
                }
            },
        };
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
