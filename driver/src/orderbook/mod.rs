use crate::contracts::{stablex_contract::StableXContract, Web3};
use crate::error::DriverError;
use crate::models::{AccountState, Order};

#[cfg(test)]
use mockall::automock;
use paginated_auction_data_reader::PaginatedAuctionDataReader;
use web3::futures::Future;
use web3::types::U256;

mod filtered_orderbook;
mod paginated_auction_data_reader;
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
pub struct PaginatedStableXOrderBookReader<'a> {
    contract: &'a dyn StableXContract,
    page_size: u64,
    web3: &'a Web3,
}

impl<'a> PaginatedStableXOrderBookReader<'a> {
    pub fn new(contract: &'a dyn StableXContract, page_size: u64, web3: &'a Web3) -> Self {
        Self {
            contract,
            page_size,
            web3,
        }
    }
}

impl<'a> StableXOrderBookReading for PaginatedStableXOrderBookReader<'a> {
    fn get_auction_index(&self) -> Result<U256> {
        self.contract
            .get_current_auction_index()
            .map(|batch_collecting_orders| batch_collecting_orders - 1)
    }

    fn get_auction_data(&self, index: U256) -> Result<(AccountState, Vec<Order>)> {
        let block = self.web3.eth().block_number().wait()?.as_u64();
        let mut reader = PaginatedAuctionDataReader::new(index);
        loop {
            let number_of_orders = reader.apply_page(&self.contract.get_auction_data_paginated(
                block,
                self.page_size,
                reader.pagination().previous_page_user,
                reader.pagination().previous_page_user_offset as u64,
            )?);
            if (number_of_orders as u64) < self.page_size {
                return Ok(reader.get_auction_data());
            }
        }
    }
}
