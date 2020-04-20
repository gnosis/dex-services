use crate::contracts::stablex_contract::StableXContract;
use crate::models::{AccountState, Order};

use anyhow::Result;
use auction_data_reader::PaginatedAuctionDataReader;
use ethcontract::U256;
#[cfg(test)]
use mockall::automock;
use std::convert::TryInto;

mod auction_data_reader;
mod filtered_orderbook;
pub use filtered_orderbook::{FilteredOrderbookReader, OrderbookFilter};

#[cfg_attr(test, automock)]
pub trait StableXOrderBookReading {
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
    fn get_auction_data(&self, index: U256) -> Result<(AccountState, Vec<Order>)> {
        let mut reader = PaginatedAuctionDataReader::new(index, self.page_size as usize);
        while let Some(pagination) = reader.pagination() {
            let page = &self.contract.get_auction_data_paginated(
                self.page_size,
                pagination.previous_page_user,
                pagination
                    .previous_page_user_offset
                    .try_into()
                    .expect("user cannot have more than u16::MAX orders"),
            )?;
            reader.apply_page(page);
        }
        Ok(reader.get_auction_data())
    }
}
