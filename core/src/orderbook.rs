mod filtered_orderbook;
pub mod streamed;
mod util;

pub use self::{
    filtered_orderbook::{FilteredOrderbookReader, OrderbookFilter},
    streamed::Orderbook as EventBasedOrderbook,
};
use crate::models::{AccountState, Order};
use anyhow::Result;
use ethcontract::BlockNumber;
use futures::future::BoxFuture;

#[cfg_attr(test, mockall::automock)]
pub trait StableXOrderBookReading: Send + Sync {
    /// Returns the current state of the order book, including account balances
    /// and open orders or an error in case it cannot get this information.
    ///
    /// # Arguments
    /// * `batch_id_to_solve` - the index for which returned orders should be valid
    fn get_auction_data_for_batch<'a>(
        &'a self,
        batch_id_to_solve: u32,
    ) -> BoxFuture<'a, Result<(AccountState, Vec<Order>)>>;

    /// Returns the state of the open orderbook at the specified block number.
    ///
    /// The open orderbook contains orders and balances that are valid for the
    /// current batch at the given block.
    fn get_auction_data_for_block<'a>(
        &'a self,
        block_number: BlockNumber,
    ) -> BoxFuture<'a, Result<(AccountState, Vec<Order>)>>;

    /// Perform potential heavy initialization of the orderbook. If this fails or wasn't called
    /// the orderbook will initialize on first use of `get_auction_data_*`.
    fn initialize<'a>(&'a self) -> BoxFuture<'a, Result<()>> {
        immediate!(Ok(()))
    }
}

/// Always suceeds with empty orderbook.
pub struct NoopOrderbook;

impl StableXOrderBookReading for NoopOrderbook {
    fn get_auction_data_for_batch(&self, _: u32) -> BoxFuture<Result<(AccountState, Vec<Order>)>> {
        immediate!(Ok(Default::default()))
    }

    fn get_auction_data_for_block(
        &self,
        _: BlockNumber,
    ) -> BoxFuture<Result<(AccountState, Vec<Order>)>> {
        immediate!(Ok(Default::default()))
    }
}
