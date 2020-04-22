use super::*;
use crate::contracts::stablex_contract::batch_exchange;
use anyhow::{anyhow, bail, Result};
use block_timestamp::BlockTimestamp;
use ethcontract::{contract::Event, errors::ExecutionError};
use futures::stream::{BoxStream, StreamExt as _};
use orderbook::Orderbook;
use std::future::Future;
use std::sync::{mpsc, Arc, Mutex};

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
    exit_indicator: mpsc::Receiver<()>,
    past_events: impl Future<Output = Result<Vec<Event<batch_exchange::Event>>, ExecutionError>>,
    mut stream: BoxStream<'static, Result<Event<batch_exchange::Event>, ExecutionError>>,
) -> Result<()> {
    let should_exit = || {
        matches!(
            exit_indicator.try_recv(),
            Err(mpsc::TryRecvError::Disconnected)
        )
    };

    for event in past_events.await? {
        handle_event(&orderbook, &mut block_timestamp, event).await?;
    }

    while !should_exit() {
        let event = stream
            .next()
            .await
            .ok_or_else(|| anyhow!("stream ended"))??;
        handle_event(&orderbook, &mut block_timestamp, event).await?;
    }

    Ok(())
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
