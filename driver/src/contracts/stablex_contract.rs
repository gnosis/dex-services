#[cfg(test)]
extern crate mock_it;

use web3::types::{H256, U128, U256};

use crate::error::DriverError;

use super::base_contract::BaseContract;

type Result<T> = std::result::Result<T, DriverError>;

#[allow(dead_code)] // event_loop needs to be retained to keep web3 connection open
struct StableXContractImpl {
    contract: BaseContract
}

#[allow(dead_code)] // event_loop needs to be retained to keep web3 connection open
impl StableXContractImpl {
    pub fn new(contract: BaseContract) -> Self {
        // Should we assert that the contract is indeed a StableX contract?
        StableXContractImpl {
            contract
        }
    }
}

pub trait StableXContract {
    fn get_current_auction_index(&self) -> Result<U256>;
    // TODO - auction_data should parse and return relevant orders and account balances.
    fn get_auction_data(&self, _index: u32) -> Result<H256>;

    fn submit_solution(
        &self,
        _batch_index: u32,
        _owners: Vec<H256>,
        _order_ids: Vec<u16>,
        _volumes: Vec<U128>,
        _prices: Vec<U128>,
        _token_ids_for_price: Vec<u16>,
    ) -> Result<()>;
}

impl StableXContract for StableXContractImpl {
    fn get_current_auction_index(&self) -> Result<U256> {
        unimplemented!();
    }

    fn get_auction_data(&self, _index: u32) -> Result<H256> {
        unimplemented!();
    }

    fn submit_solution(
        &self,
        _batch_index: u32,
        _owners: Vec<H256>,
        _order_ids: Vec<u16>,
        _volumes: Vec<U128>,
        _prices: Vec<U128>,
        _token_ids_for_price: Vec<u16>,
    ) -> Result<()> {
        unimplemented!();
    }
}

#[cfg(test)]
pub mod tests {
    use mock_it::Matcher;
    use mock_it::Matcher::*;
    use mock_it::Mock;

    use crate::error::ErrorKind;

    use super::*;

    type SubmitSolutionArguments = (
        u32,
        Matcher<Vec<H256>>,
        Matcher<Vec<u16>>,
        Matcher<Vec<U128>>,
        Matcher<Vec<U128>>,
        Matcher<Vec<u16>>,
    );

    #[derive(Clone)]
    pub struct StableXContractMock {
        pub get_current_auction_index: Mock<(), Result<U256>>,
        pub get_auction_data: Mock<u32, Result<H256>>,
        pub submit_solution: Mock<SubmitSolutionArguments, Result<()>>,
    }

    impl Default for StableXContractMock {
        fn default() -> StableXContractMock {
            StableXContractMock {
                get_current_auction_index: Mock::new(Err(DriverError::new(
                    "Unexpected call to get_current_auction_index",
                    ErrorKind::Unknown,
                ))),
                get_auction_data: Mock::new(Err(DriverError::new(
                    "Unexpected call to get_auction_data",
                    ErrorKind::Unknown,
                ))),
                submit_solution: Mock::new(Err(DriverError::new(
                    "Unexpected call to submit_solution",
                    ErrorKind::Unknown,
                ))),
            }
        }
    }

    impl StableXContract for StableXContractMock {
        fn get_current_auction_index(&self) -> Result<U256> {
            self.get_current_auction_index.called(())
        }
        fn get_auction_data(&self, index: u32) -> Result<H256> {
            self.get_auction_data.called(index)
        }
        fn submit_solution(
            &self,
            batch_index: u32,
            owners: Vec<H256>,
            order_ids: Vec<u16>,
            volumes: Vec<U128>,
            prices: Vec<U128>,
            token_ids_for_price: Vec<u16>,
        ) -> Result<()> {
            self.submit_solution.called(
                (
                    batch_index,
                    Val(owners),
                    Val(order_ids),
                    Val(volumes),
                    Val(prices),
                    Val(token_ids_for_price)
                )
            )
        }
    }
}
