mod auction_data_reader;
mod filtered_orderbook;
mod onchain_filtered_orderbook;
mod paginated_orderbook;
mod shadow_orderbook;
pub mod streamed;
mod util;

pub use self::filtered_orderbook::{FilteredOrderbookReader, OrderbookFilter};
pub use self::onchain_filtered_orderbook::OnchainFilteredOrderBookReader;
pub use self::paginated_orderbook::PaginatedStableXOrderBookReader;
pub use self::shadow_orderbook::ShadowedOrderbookReader;
pub use self::streamed::Orderbook as EventBasedOrderbook;

use crate::contracts::{stablex_contract::StableXContractImpl, Web3};
use crate::models::{AccountState, Order};

use anyhow::{anyhow, Error, Result};
use futures::future::{BoxFuture, FutureExt as _};
#[cfg(test)]
use mockall::automock;
use std::path::PathBuf;
use std::str::FromStr;
use std::sync::Arc;

#[cfg_attr(test, automock)]
pub trait StableXOrderBookReading: Send + Sync {
    /// Returns the current state of the order book, including account balances
    /// and open orders or an error in case it cannot get this information.
    ///
    /// # Arguments
    /// * `batch_id_to_solve` - the index for which returned orders should be valid
    fn get_auction_data<'a>(
        &'a self,
        batch_id_to_solve: u32,
    ) -> BoxFuture<'a, Result<(AccountState, Vec<Order>)>>;
    /// Perform potential heavy initialization of the orderbook. If this fails or wasn't called
    // the orderbook will initialize on first use of `get_auction_data`.
    fn initialize<'a>(&'a self) -> BoxFuture<'a, Result<()>> {
        async { Ok(()) }.boxed()
    }
}

/// The different kinds of orderbook readers.
#[derive(Clone, Debug)]
pub enum OrderbookReaderKind {
    /// An unfiltered paginated orderbook read directly from the EVM
    Paginated,
    /// A paginated orderbook read from and filtered by EVM
    OnchainFiltered,
    /// An orderbook reader that is built from subscribing to
    /// relevant ethereum events emitted by the exchange contract
    EventBased,
}

impl OrderbookReaderKind {
    /// Creates a new Orderbook reader based on the parameters.
    pub fn create(
        &self,
        contract: Arc<StableXContractImpl>,
        auction_data_page_size: u16,
        orderbook_filter: &OrderbookFilter,
        web3: Web3,
        file_path: Option<PathBuf>,
    ) -> Box<dyn StableXOrderBookReading> {
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
            OrderbookReaderKind::EventBased => Box::new(EventBasedOrderbook::new(
                contract,
                web3,
                auction_data_page_size as _,
                file_path,
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
            "eventbased" => Ok(OrderbookReaderKind::EventBased),
            _ => Err(anyhow!("unknown orderbook reader kind '{}'", value)),
        }
    }
}
