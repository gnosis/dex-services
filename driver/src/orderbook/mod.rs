use crate::contracts::stablex_contract::StableXContract;
use crate::error::DriverError;

use dfusion_core::models::{AccountState, Order};

use web3::types::U256;

type Result<T> = std::result::Result<T, DriverError>;

pub trait StableXOrderBookReading {
    /// Retunrs the index of the auction that is currently being solved
    /// or an error in case it cannot get this information.
    fn get_auction_index(&self) -> Result<U256>;

    /// Returns the current state of the order book, inclducing account balances
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

#[cfg(test)]
pub mod tests {
    use super::*;
    use crate::error::ErrorKind;
    use mock_it::Mock;

    #[derive(Clone)]
    pub struct StableXOrderBookReadingMock {
        pub get_auction_index: Mock<(), Result<U256>>,
        pub get_auction_data: Mock<U256, Result<(AccountState, Vec<Order>)>>,
    }

    impl Default for StableXOrderBookReadingMock {
        fn default() -> Self {
            Self {
                get_auction_index: Mock::new(Err(DriverError::new(
                    "Unexpected call to get_auction_index",
                    ErrorKind::Unknown,
                ))),
                get_auction_data: Mock::new(Err(DriverError::new(
                    "Unexpected call to get_auction_data",
                    ErrorKind::Unknown,
                ))),
            }
        }
    }

    impl StableXOrderBookReading for StableXOrderBookReadingMock {
        fn get_auction_index(&self) -> Result<U256> {
            self.get_auction_index.called(())
        }
        fn get_auction_data(&self, index: U256) -> Result<(AccountState, Vec<Order>)> {
            self.get_auction_data.called(index)
        }
    }
}
