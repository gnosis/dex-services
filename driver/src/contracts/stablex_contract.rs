#[cfg(test)]
extern crate mock_it;

use dfusion_core::models::{AccountState, Order};

// TODO - uncomment when removing "unimplemented!()"
//use web3::contract::Options;
//use web3::futures::Future;
use web3::types::{H160, U128};

use crate::error::DriverError;

use super::base_contract::BaseContract;

use std::env;
use std::fs;

type Result<T> = std::result::Result<T, DriverError>;

#[allow(dead_code)]
struct StableXContractImpl {
    base: BaseContract
}

#[allow(dead_code)]
impl StableXContractImpl {
    pub fn new() -> Result<Self> {
        let contract_json = fs::read_to_string("dex-contracts/build/contracts/StablecoinConverter.json").unwrap();
        let address = env::var("STABLEX_CONTRACT_ADDRESS").unwrap();
        Ok(
            StableXContractImpl {
                base: BaseContract::new(address, contract_json).unwrap()
            }
        )
    }
}

pub trait StableXContract {
    fn get_current_auction_index(&self) -> Result<u32>;
    fn get_auction_data(&self, _index: u32) -> Result<(AccountState, Vec<Order>)>;

    fn submit_solution(
        &self,
        _batch_index: u32,
        _owners: Vec<H160>,
        _order_ids: Vec<u16>,
        _volumes: Vec<U128>,
        _prices: Vec<U128>,
        _token_ids_for_price: Vec<u16>,
    ) -> Result<()>;
}

impl StableXContract for StableXContractImpl {
    fn get_current_auction_index(&self) -> Result<u32> {
        unimplemented!();
//        self.base.contract
//            .query("getCurrentStateIndex", (), None, Options::default(), None)
//            .wait()
//            .map_err(DriverError::from)
    }

    fn get_auction_data(&self, _index: u32) -> Result<(AccountState, Vec<Order>)> {
        unimplemented!();
    }

    fn submit_solution(
        &self,
        _batch_index: u32,
        _owners: Vec<H160>,
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
        Matcher<Vec<H160>>,
        Matcher<Vec<u16>>,
        Matcher<Vec<U128>>,
        Matcher<Vec<U128>>,
        Matcher<Vec<u16>>,
    );

    #[derive(Clone)]
    pub struct StableXContractMock {
        pub get_current_auction_index: Mock<(), Result<u32>>,
        pub get_auction_data: Mock<u32, Result<(AccountState, Vec<Order>)>>,
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
        fn get_current_auction_index(&self) -> Result<u32> {
            self.get_current_auction_index.called(())
        }
        fn get_auction_data(&self, index: u32) -> Result<(AccountState, Vec<Order>)> {
            self.get_auction_data.called(index)
        }
        fn submit_solution(
            &self,
            batch_index: u32,
            owners: Vec<H160>,
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
