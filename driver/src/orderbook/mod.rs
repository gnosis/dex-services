use crate::contracts::stablex_contract::StableXContract;
use crate::error::DriverError;

use dfusion_core::models::{AccountState, Order};
#[cfg(test)]
use mockall::automock;

use web3::types::U256;

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

/// A simple implementation of the order book trait that forwards calls
/// directly to the smart contract.
pub struct StableXOrderBookReader<'a> {
    contract: &'a dyn StableXContract,
}

impl<'a> StableXOrderBookReader<'a> {
    pub fn new(contract: &'a dyn StableXContract) -> Self {
        Self { contract }
    }
}

impl<'a> StableXOrderBookReading for StableXOrderBookReader<'a> {
    fn get_auction_index(&self) -> Result<U256> {
        self.contract
            .get_current_auction_index()
            .map(|batch_collecting_orders| batch_collecting_orders - 1)
    }

    fn get_auction_data(&self, index: U256) -> Result<(AccountState, Vec<Order>)> {
        self.contract.get_auction_data(index)
    }
}

/// Implements the StableXOrderBookReading trait by using the underlying
/// contract in a paginated way.
/// This avoid hitting gas limits when the total amount of orders is large.
pub struct PaginatedStableXOrderBookReader<'a> {
    contract: &'a dyn StableXContract,
}

impl<'a> StableXOrderBookReading for PaginatedStableXOrderBookReader<'a> {
    fn get_auction_index(&self) -> Result<U256> {
        StableXOrderBookReader::new(self.contract).get_auction_index()
    }

    fn get_auction_data(&self, index: U256) -> Result<(AccountState, Vec<Order>)> {
        self.contract.get_auction_data_batched(index)
    }
}
