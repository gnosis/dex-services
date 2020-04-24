mod auction_data_reader;
mod filtered_orderbook;
mod onchain_filtered_orderbook;
mod paginated_orderbook;
mod shadow_orderbook;

pub use self::filtered_orderbook::{FilteredOrderbookReader, OrderbookFilter};
pub use self::onchain_filtered_orderbook::OnchainFilteredOrderBookReader;
pub use self::paginated_orderbook::PaginatedStableXOrderBookReader;
pub use self::shadow_orderbook::ShadowedOrderbookReader;

use crate::contracts::stablex_contract::StableXContractImpl;
use crate::models::{AccountState, Order};
use anyhow::{anyhow, Error, Result};
use ethcontract::U256;
#[cfg(test)]
use mockall::automock;
use std::str::FromStr;
use std::sync::Arc;

#[cfg_attr(test, automock)]
pub trait StableXOrderBookReading {
    /// Returns the current state of the order book, including account balances
    /// and open orders or an error in case it cannot get this information.
    ///
    /// # Arguments
    /// * `index` - the auction index for which returned orders should be valid
    fn get_auction_data(&self, index: U256) -> Result<(AccountState, Vec<Order>)>;
}

/// The different kinds of orderbook readers.
#[derive(Debug)]
pub enum OrderbookReaderKind {
    /// An unfiltered paginated orderbook read directly from the EVM
    Paginated,
    /// A paginated orderbook reader read from and filtered by EVM
    OnchainFiltered,
}

impl OrderbookReaderKind {
    /// Creates a new Orderbook reader based on the parameters.
    pub fn create(
        &self,
        contract: Arc<StableXContractImpl>,
        auction_data_page_size: u16,
        orderbook_filter: &OrderbookFilter,
    ) -> Box<dyn StableXOrderBookReading + Sync> {
        match self {
            OrderbookReaderKind::Paginated => Box::new(PaginatedStableXOrderBookReader::new(
                contract,
                auction_data_page_size,
            )),
            OrderbookReaderKind::OnchainFiltered => Box::new(OnchainFilteredOrderBookReader::new(
                contract,
                auction_data_page_size,
                orderbook_filter,
            )),
        }
    }
}

impl FromStr for OrderbookReaderKind {
    type Err = Error;
    fn from_str(value: &str) -> Result<Self> {
        match value.to_lowercase().as_str() {
            "paginated" => Ok(OrderbookReaderKind::Paginated),
            "onchainfiltered" => Ok(OrderbookReaderKind::OnchainFiltered),
            _ => Err(anyhow!("unknown orderbook reader kind '{}'", value)),
        }
    }
}
