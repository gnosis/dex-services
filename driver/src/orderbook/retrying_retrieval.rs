//! This module implements a retrying orderbook reader that accounts for
//! submitted solutions as well as batches rolling over while the orderbook is
//! being read.

#![allow(dead_code)]

use crate::contracts::stablex_contract::batch_exchange;
use crate::contracts::stablex_contract::batch_exchange::event_data::{SolutionSubmission, Trade};
use crate::models::{AccountState, Order};
use crate::orderbook::paginated_auction_data_reader::PaginatedAuctionDataReader;
use anyhow::{anyhow, Result};
use ethcontract::web3::types::BlockId;
use ethcontract::{Address, Event, EventData, H256};
use futures::channel::{mpsc, oneshot};
use futures::future::{BoxFuture, FutureExt};
use futures::stream::{BoxStream, StreamExt};
use futures::{select, try_join};
use std::collections::HashMap;
use std::convert::TryInto;

/// Type definition for complete solution data.
pub type SolutionData = (SolutionSubmission, Vec<Trade>);

/// A trait for querying contract data with futures and streams.
#[cfg_attr(test, mockall::automock)]
// NOTE: Clippy complains that the lifetime can be elided with `'_` for the
//   various streams and futures, but doing so results in a compiler errors.
#[allow(clippy::needless_lifetimes)]
pub trait StableXContractAsync: Sync {
    /// Retrieves the batch for a specific block number.
    fn batch_id_at_block<'a>(&'a self, block: BlockId) -> BoxFuture<'a, Result<u32>>;
    /// Retrieves the latest solution for the given batch index.
    fn latest_solution<'a>(&'a self, batch_id: u32) -> BoxFuture<'a, Result<Option<SolutionData>>>;
    /// Streams pages of encoded orders.
    fn encoded_orders_paginated<'a>(
        &'a self,
        previous_page_user: Address,
        previous_page_user_offset: u16,
        page_size: u16,
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
        let (retry_tx, retry_rx) = mpsc::channel::<()>(0);
        let (done_tx, done_rx) = oneshot::channel::<()>();

        let (solution_event, latest_solution, (account_state, orders)) = try_join!(
            poll_incoming_events(contract, batch_id, retry_tx, done_rx),
            contract.latest_solution(batch_id),
            read_orderbook_async(contract, batch_id, page_size, retry_rx, done_tx),
        )?;

        if let Some((solution, trades)) = solution_event.or(latest_solution) {
            todo!("undo {:?} and from {:?} from orderbook", solution, trades);
        }

        Ok((account_state, orders))
    })
}

async fn read_orderbook_async(
    contract: &dyn StableXContractAsync,
    batch_id: u32,
    page_size: u16,
    mut retry: mpsc::Receiver<()>,
    _done: oneshot::Sender<()>,
) -> Result<(AccountState, Vec<Order>)> {
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
            )
            .fuse();

        let page = select! {
            page = page_future => page?,
            _ = retry.next() => {
                reader = PaginatedAuctionDataReader::new(batch_id.into());
                continue;
            },
            complete => break Err(anyhow!("error processing incoming events")),
        };

        let number_of_orders: u16 = reader
            .apply_page(&page)
            .try_into()
            .expect("number of orders per page should never overflow a u16");

        if number_of_orders < page_size {
            return Ok(reader.get_auction_data());
        }
    }

    // NOTE: `done` gets dropped automatically when this method returns, making
    //   its `Receiver` resolve to `Err(Cancelled)` which will cause the event
    //   loop to break.
}

async fn poll_incoming_events(
    contract: &dyn StableXContractAsync,
    batch_id: u32,
    mut retry: mpsc::Sender<()>,
    done: oneshot::Receiver<()>,
) -> Result<Option<SolutionData>> {
    use batch_exchange::Event::*;
    use Batch::*;
    use EventData::*;

    let mut done = done.fuse();
    let mut events = contract.all_events().fuse();

    let mut batches = BatchIds::new(contract, batch_id);
    let mut solutions = SolutionAccumulator::default();

    loop {
        let event = select! {
            event = events.select_next_some() => event?,
            _ = done => break Ok(()),
            complete => break Err(anyhow!("event stream unexpectedly ended")),
        };

        let block_hash = match event.meta.as_ref() {
            Some(meta) => meta.block_hash,
            _ => return Err(anyhow!("unexpected event on pending block in event stream")),
        };

        let event_batch = batches.block_batch(block_hash).await?;
        let (data, added_or_removed) = match event.data {
            Added(data) => (data, Added(())),
            Removed(data) => (data, Removed(())),
        };

        let requires_retry = match (data, event_batch, added_or_removed) {
            (Deposit(deposit), _, _) if deposit.batch_id <= batch_id => true,
            (OrderCancellation(_), Past, _) => true,
            (OrderPlacement(order), _, _)
                if batch_id >= order.valid_from && batch_id <= order.valid_until =>
            {
                true
            }
            (SolutionSubmission(solution), Current, Added(_)) => {
                solutions.add_solution_submission(block_hash, solution);
                true
            }
            (SolutionSubmission(_), Current, Removed(_)) => {
                solutions.remove_solution_submission(block_hash);
                true
            }
            (Trade(trade), Current, Added(_)) => {
                solutions.add_trade(block_hash, trade);
                false
            }
            (WithdrawRequest(withdraw), _, _) if withdraw.batch_id <= batch_id => true,
            _ => false,
        };

        if requires_retry {
            match retry.try_send(()) {
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

    Ok(solutions.latest_solution())
}

struct BatchIds<'a> {
    contract: &'a dyn StableXContractAsync,
    current_batch: u32,
    block_batch_ids: HashMap<H256, u32>,
}

enum Batch {
    Past,
    Current,
}

impl<'a> BatchIds<'a> {
    fn new(contract: &'a dyn StableXContractAsync, current_batch: u32) -> Self {
        BatchIds {
            contract,
            current_batch,
            block_batch_ids: HashMap::new(),
        }
    }

    async fn block_batch(&mut self, block_hash: H256) -> Result<Batch> {
        use std::cmp::Ordering::*;

        let event_batch_id = match self.block_batch_ids.get(&block_hash) {
            Some(&id) => id,
            None => {
                let id = self.contract.batch_id_at_block(block_hash.into()).await?;
                self.block_batch_ids.insert(block_hash, id);
                id
            }
        };

        match event_batch_id.cmp(&self.current_batch) {
            Less => Ok(Batch::Past),
            Equal => Ok(Batch::Current),
            Greater => Err(anyhow!("orderbook retrieval took more than a full batch")),
        }
    }
}

#[derive(Default)]
struct SolutionAccumulator {
    solutions: HashMap<H256, SolutionData>,
    blocks: Vec<H256>,
}

impl SolutionAccumulator {
    fn add_trade(&mut self, block_hash: H256, trade: Trade) {
        self.solutions.entry(block_hash).or_default().1.push(trade)
    }

    fn add_solution_submission(&mut self, block_hash: H256, solution: SolutionSubmission) {
        self.solutions.entry(block_hash).or_default().0 = solution;
        self.blocks.push(block_hash);
    }

    fn remove_solution_submission(&mut self, block_hash: H256) {
        self.solutions.remove(&block_hash);
        if let Some(index) = self.blocks.iter().position(|&b| b == block_hash) {
            self.blocks.remove(index);
        }
    }

    fn latest_solution(mut self) -> Option<SolutionData> {
        let latest = self.blocks.pop()?;
        self.solutions.remove(&latest)
    }
}
