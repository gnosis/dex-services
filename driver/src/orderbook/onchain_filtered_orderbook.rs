use crate::contracts::stablex_contract::{FilteredOrderPage, StableXContract};
use crate::models::{AccountState, Order};

use super::auction_data_reader::IndexedAuctionDataReader;
use super::filtered_orderbook::OrderbookFilter;
use super::StableXOrderBookReading;

use anyhow::Result;
use ethcontract::{Address, U256};
use std::sync::Arc;

pub struct OnchainFilteredOrderBookReader {
    contract: Arc<dyn StableXContract + Send + Sync>,
    page_size: u16,
    filter: Vec<u16>,
}

impl OnchainFilteredOrderBookReader {
    pub fn new(
        contract: Arc<dyn StableXContract + Send + Sync>,
        page_size: u16,
        filter: &OrderbookFilter,
    ) -> Self {
        Self {
            contract,
            page_size,
            filter: filter
                .whitelist()
                .map(|set| set.iter().cloned().collect())
                .unwrap_or(vec![]),
        }
    }
}

impl StableXOrderBookReading for OnchainFilteredOrderBookReader {
    fn get_auction_data(&self, index: U256) -> Result<(AccountState, Vec<Order>)> {
        let mut reader = IndexedAuctionDataReader::new(index);
        let mut auction_data = FilteredOrderPage {
            indexed_elements: vec![],
            has_next_page: true,
            next_page_user: Address::zero(),
            next_page_user_offset: 0,
        };
        while auction_data.has_next_page {
            auction_data = self.contract.get_filtered_auction_data_paginated(
                index,
                self.filter.clone(),
                self.page_size,
                auction_data.next_page_user,
                auction_data.next_page_user_offset,
            )?;
            reader.apply_page(&auction_data.indexed_elements);
        }
        return Ok(reader.get_auction_data());
    }
}
