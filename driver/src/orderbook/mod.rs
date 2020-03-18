use crate::contracts::stablex_contract::StableXContract;
use crate::models::{AccountState, Order};

use anyhow::Result;
use ethcontract::U256;
#[cfg(test)]
use mockall::automock;
use paginated_auction_data_reader::PaginatedAuctionDataReader;
use std::convert::TryInto;

mod filtered_orderbook;
mod paginated_auction_data_reader;
pub use filtered_orderbook::FilteredOrderbookReader;
pub use filtered_orderbook::OrderbookFilter;

#[cfg_attr(test, automock)]
pub trait StableXOrderBookReading {
    /// Returns the index of the auction that is currently being solved
    /// or an error in case it cannot get this information.
    fn get_auction_index(&self) -> Result<u32>;

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
    contract: &'a (dyn StableXContract + Sync),
    page_size: u16,
}

impl<'a> PaginatedStableXOrderBookReader<'a> {
    pub fn new(contract: &'a (dyn StableXContract + Sync), page_size: u16) -> Self {
        Self {
            contract,
            page_size,
        }
    }
}

impl<'a> StableXOrderBookReading for PaginatedStableXOrderBookReader<'a> {
    fn get_auction_index(&self) -> Result<u32> {
        self.contract
            .get_current_auction_index()
            .map(|batch_collecting_orders| batch_collecting_orders - 1)
    }

    fn get_auction_data(&self, index: U256) -> Result<(AccountState, Vec<Order>)> {
        let mut reader = PaginatedAuctionDataReader::new(index);
        loop {
            let number_of_orders: u16 = reader
                .apply_page(
                    &self.contract.get_auction_data_paginated(
                        self.page_size,
                        reader.pagination().previous_page_user,
                        reader
                            .pagination()
                            .previous_page_user_offset
                            .try_into()
                            .expect("user cannot have more than u16::MAX orders"),
                    )?,
                )
                .try_into()
                .expect("number of orders per page should never overflow a u16");
            if number_of_orders < self.page_size {
                return Ok(reader.get_auction_data());
            }
        }
    }
}
