//! This module implements a retrying orderbook reader that accounts that allows
//! the orderbook retrieval to optimistically start before the batch is
//! finalized and will retry if state invalidating events are emitted.

#![allow(dead_code)]

use crate::contracts::stablex_contract::batch_exchange;
use crate::models::{AccountState, Order};
use crate::orderbook::paginated_auction_data_reader::PaginatedAuctionDataReader;
use anyhow::{anyhow, Result};
use ethcontract::web3::types::BlockId;
use ethcontract::{Address, BlockNumber, Event, EventData};
use futures::channel::{mpsc, oneshot};
use futures::future::{BoxFuture, FutureExt};
use futures::stream::{BoxStream, StreamExt};
use futures::{select, try_join};
use std::convert::TryInto;

/// A trait for querying contract data with futures and streams.
#[cfg_attr(test, mockall::automock)]
#[allow(clippy::needless_lifetimes)]
pub trait StableXContractAsync: Sync {
    /// Retrieves the batch for a specific block number.
    fn batch_id_at_block<'a>(&'a self, block: BlockId) -> BoxFuture<'a, Result<u32>>;
    /// Searches for the block number of the last block of the given batch. If
    /// the batch has not yet been finalized, then the current block number is
    /// returned.
    fn last_block_for_batch<'a>(&'a self, batch_id: u32) -> BoxFuture<'a, Result<u64>>;
    /// Retrieves the latest solution for the given batch index.
    fn encoded_orders_paginated<'a>(
        &'a self,
        previous_page_user: Address,
        previous_page_user_offset: u16,
        page_size: u16,
        block_number: BlockNumber,
    ) -> BoxFuture<'a, Result<Vec<u8>>>;
    /// Streams incomming events from the contract that may signal that the
    /// orderbook retrieval must be retried as the data has become stale.
    fn all_events<'a>(&'a self) -> BoxStream<'a, Result<Event<batch_exchange::Event>>>;
}

fn read_orderbook(
    contract: &dyn StableXContractAsync,
    batch_id: u32,
    page_size: u16,
) -> Result<(AccountState, Vec<Order>)> {
    futures::executor::block_on(async {
        // NOTE: Use a bounded channel with a 0 sized buffer so that the sender
        //   (i.e. the future that is polling the incomming events) can only
        //   send 1 retry at a time.
        let (retry_tx, retry_rx) = mpsc::channel::<u64>(0);
        // NOTE: Use a one shot channel to signal when the orderbook retrieval
        //   is complete so that the event watcher future can end.
        let (done_tx, done_rx) = oneshot::channel::<()>();

        let (_, (account_state, orders)) = try_join!(
            watch_contract_events(contract, batch_id, retry_tx, done_rx),
            async move {
                let result = read_orderbook_at_block(contract, batch_id, page_size, retry_rx).await;
                // NOTE: drop `done` sender so that the event watcher future
                // exits gracefully.
                drop(done_tx);
                result
            }
        )?;

        Ok((account_state, orders))
    })
}

/// Read an orderbook at a given block. This method takes an additional `retry`
/// channel that is used to signal when the orderbook retrieval needs to be
/// restarted on a new block number because new events have arrived indicating
/// that the retrieved orderbook may be stale and produce invalid solutions.
///
/// Note this method is asynchronous so that it can simultaneously poll both
/// incomming retry signals and the oderbook pages and restart querying the
/// orderbook as soon as possible.
async fn read_orderbook_at_block(
    contract: &dyn StableXContractAsync,
    batch_id: u32,
    page_size: u16,
    mut retry: mpsc::Receiver<u64>,
) -> Result<(AccountState, Vec<Order>)> {
    // NOTE: First get the block used to query the orderbook. This is either
    //   the last block of the batch, or a new block number
    let mut last_block_future = contract.last_block_for_batch(batch_id).fuse();
    let mut block_number = select! {
        block = last_block_future => block?,
        block = retry.select_next_some() => block,
    };

    let mut reader = PaginatedAuctionDataReader::new(batch_id.into());
    loop {
        let mut page_future = contract
            .encoded_orders_paginated(
                reader.pagination().previous_page_user,
                reader
                    .pagination()
                    .previous_page_user_offset
                    .try_into()
                    .expect("user cannot have more than u16::MAX orders"),
                page_size,
                block_number.into(),
            )
            .fuse();

        let page = select! {
            page = page_future => page?,
            block = retry.next() => {
                block_number = match block {
                    Some(block) => block,
                    None => {
                        return Err(anyhow!("retry channel unexpectedly closed"));
                    }
                };
                reader = PaginatedAuctionDataReader::new(batch_id.into());
                continue;
            },
        };

        let number_of_orders: u16 = reader
            .apply_page(&page)
            .try_into()
            .expect("number of orders per page should never overflow a u16");

        if number_of_orders < page_size {
            return Ok(reader.get_auction_data());
        }
    }
}

/// Watches exchange contract events for events that would invalidate an
/// orderbook.
///
/// This event uses a `retry` channel to signal the most accurate block number
/// to use to re-query the orderbook in case an event is encoutered that would
/// otherwise invalidate the orderbook
///
/// Note this takes an additional `done` channel that can be used to request
/// this future to end. If the done channel is never signaled, then the future
/// returned by this method will not resolve unless it encounters an error.
async fn watch_contract_events(
    contract: &dyn StableXContractAsync,
    batch_id: u32,
    mut retry: mpsc::Sender<u64>,
    done: oneshot::Receiver<()>,
) -> Result<()> {
    use batch_exchange::Event::*;
    use EventData::*;

    let mut done = done.fuse();
    let mut events = contract.all_events().fuse();

    loop {
        // NOTE: Wait for the next event. Simultaneously poll the done channel
        //   to check to see if the caller requested us to exit.
        let event = select! {
            event = events.next() => match event.transpose()? {
                Some(event) => event,
                None => break Err(anyhow!("event stream unexpectedly ended")),
            },
            _ = done => break Ok(()),
        };

        let block_number = match event.meta.as_ref() {
            Some(meta) => meta.block_number,
            _ => return Err(anyhow!("unexpected event on pending block in event stream")),
        };

        let requires_retry = match &event.data {
            // NOTE: A deposit or withdrawal was either added or removed as a
            //   result of a reorg. That means that a user balance may have been
            //   retrieved with an incorrect value and may cause an invalid
            //   solution either by generating more negative utility or using a
            //   larger amount than is permitted.
            Added(Deposit(deposit)) | Removed(Deposit(deposit)) => deposit.batch_id <= batch_id,
            Added(WithdrawRequest(withdraw)) | Removed(WithdrawRequest(withdraw)) => {
                withdraw.batch_id <= batch_id
            }
            // NOTE: A new order cancellation event was emitted, this means that
            //   a potentially invalid event was included in the account state
            //   that can lead to invalid solutions. Note that if the event was
            //   removed, then we are missing a valid order which means the
            //   solution may be suboptimal but it is not worth retrying.
            Added(OrderCancellation(_)) => {
                let event_batch_id = contract
                    .batch_id_at_block(BlockId::Number(block_number.into()))
                    .await?;
                event_batch_id <= batch_id
            }
            // NOTE: The reciprocal of an order cancellation. Note that new
            //   orders are ignored, as they cannot cause invalid solutions.
            Removed(OrderPlacement(order)) => {
                batch_id >= order.valid_from && batch_id <= order.valid_until
            }
            _ => false,
        };

        if requires_retry {
            // NOTE: For removed events, this means that the previous block is
            //   now the most accurate block to use for querying the orderbook.
            let retry_block_number = match &event.data {
                Added(_) => block_number,
                Removed(_) => block_number - 1,
            };
            match retry.try_send(retry_block_number) {
                Ok(()) => {}
                Err(err) if err.is_full() => {
                    // This indicates that our buffered channel already has
                    // another pending retry, so there is no use in adding
                    // another.
                }
                Err(err) => {
                    return Err(err.into());
                }
            }
        }
    }?;

    Ok(())
}
