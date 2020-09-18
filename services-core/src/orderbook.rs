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

#[cfg_attr(test, mockall::automock)]
#[async_trait::async_trait]
pub trait StableXOrderBookReading: Send + Sync {
    /// Returns the current state of the order book, including account balances
    /// and open orders or an error in case it cannot get this information.
    ///
    /// # Arguments
    /// * `batch_id_to_solve` - the index for which returned orders should be valid
    async fn get_auction_data_for_batch(
        &self,
        batch_id_to_solve: u32,
    ) -> Result<(AccountState, Vec<Order>)>;

    /// Returns the state of the open orderbook at the specified block number.
    ///
    /// The open orderbook contains orders and balances that are valid for the
    /// current batch at the given block.
    async fn get_auction_data_for_block(
        &self,
        block_number: BlockNumber,
    ) -> Result<(AccountState, Vec<Order>)>;

    /// Perform potential heavy initialization of the orderbook. If this fails or wasn't called
    /// the orderbook will initialize on first use of `get_auction_data_*`.
    async fn initialize(&self) -> Result<()> {
        Ok(())
    }
}

/// Always suceeds with empty orderbook.
pub struct NoopOrderbook;

#[async_trait::async_trait]
impl StableXOrderBookReading for NoopOrderbook {
    async fn get_auction_data_for_batch(&self, _: u32) -> Result<(AccountState, Vec<Order>)> {
        Ok(Default::default())
    }

    async fn get_auction_data_for_block(
        &self,
        _: BlockNumber,
    ) -> Result<(AccountState, Vec<Order>)> {
        Ok(Default::default())
    }
}
