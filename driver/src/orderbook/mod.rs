use crate::contracts::{stablex_contract::StableXContract, Web3};
use crate::error::DriverError;
use crate::models::{AccountState, Order};

use batched_auction_data_reader::BatchedAuctionDataReader;
#[cfg(test)]
use mockall::automock;
use web3::futures::Future;
use web3::types::U256;

mod batched_auction_data_reader;
mod filtered_orderbook;
pub use filtered_orderbook::FilteredOrderbookReader;
pub use filtered_orderbook::OrderbookFilter;

type Result<T> = std::result::Result<T, DriverError>;

#[cfg_attr(test, automock)]
pub trait StableXOrderBookReading {
    /// Returns the index of the auction that is currently being solved
    /// or an error in case it cannot get this information.
    fn get_auction_index(&self) -> Result<U256>;

    /// Returns the current state of the order book, including account balances
    /// and open orders or an error in case it cannot get this information.
    ///
    /// # Arguments
    /// * `index` - the auction index for which returned orders should be valid
    fn get_auction_data(&self, index: U256) -> Result<(AccountState, Vec<Order>)>;
}

/// Implements the StableXOrderBookReading trait by using the underlying
/// contract in a paginated way.
/// This avoid hitting gas limits when the total amount of orders is large.
pub struct PaginatedStableXOrderBookReader<'a, 'b> {
    contract: &'a dyn StableXContract,
    page_size: u64,
    web3: &'b Web3,
}

impl<'a, 'b> PaginatedStableXOrderBookReader<'a, 'b> {
    pub fn new(contract: &'a dyn StableXContract, page_size: u64, web3: &'b Web3) -> Self {
        Self {
            contract,
            page_size,
            web3,
        }
    }
}

impl<'a, 'b> StableXOrderBookReading for PaginatedStableXOrderBookReader<'a, 'b> {
    fn get_auction_index(&self) -> Result<U256> {
        self.contract
            .get_current_auction_index()
            .map(|batch_collecting_orders| batch_collecting_orders - 1)
    }

    fn get_auction_data(&self, index: U256) -> Result<(AccountState, Vec<Order>)> {
        let block = self.web3.eth().block_number().wait()?.as_u64();
        let mut reader = BatchedAuctionDataReader::new(index);
        loop {
            let number_of_added_orders =
                reader.apply_batch(&self.contract.get_auction_data_batched(
                    block,
                    self.page_size,
                    reader.pagination().previous_page_user,
                    reader.pagination().previous_page_user_offset as u64,
                )?);
            if (number_of_added_orders as u64) < self.page_size {
                let done = reader.done();
                return Ok((done.account_state, done.orders));
            }
        }
    }
}
