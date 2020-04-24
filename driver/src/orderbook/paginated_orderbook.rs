use crate::contracts::stablex_contract::StableXContract;
use crate::models::{AccountState, Order};

use super::auction_data_reader::PaginatedAuctionDataReader;
use super::StableXOrderBookReading;
use anyhow::Result;
use ethcontract::{BlockNumber, U256};
use std::convert::TryInto;
use std::sync::Arc;

/// Implements the StableXOrderBookReading trait by using the underlying
/// contract in a paginated way.
/// This avoid hitting gas limits when the total amount of orders is large.
pub struct PaginatedStableXOrderBookReader {
    contract: Arc<dyn StableXContract + Send + Sync>,
    page_size: u16,
}

impl PaginatedStableXOrderBookReader {
    pub fn new(contract: Arc<dyn StableXContract + Send + Sync>, page_size: u16) -> Self {
        Self {
            contract,
            page_size,
        }
    }
}

impl StableXOrderBookReading for PaginatedStableXOrderBookReader {
    fn get_auction_data(&self, index: U256) -> Result<(AccountState, Vec<Order>)> {
        let mut reader = PaginatedAuctionDataReader::new(index, self.page_size as usize);
        while let Some(page_info) = reader.next_page() {
            let page = &self.contract.get_auction_data_paginated(
                self.page_size,
                page_info.previous_page_user,
                page_info
                    .previous_page_user_offset
                    .try_into()
                    .expect("user cannot have more than u16::MAX orders"),
                Some(BlockNumber::Pending),
            )?;
            reader.apply_page(page);
        }
        Ok(reader.get_auction_data())
    }
}
