#[cfg(test)]
extern crate mock_it;

use dfusion_core::models::{AccountState, Order};

use web3::contract::Options;
use web3::futures::Future;
use web3::types::{H160, U128, U256};

use crate::error::DriverError;

use super::base_contract::BaseContract;

use std::env;
use std::fs;

type Result<T> = std::result::Result<T, DriverError>;

#[allow(dead_code)] // TODO - remove once used
struct StableXContractImpl {
    base: BaseContract
}

#[allow(dead_code)] // TODO - remove once used
impl StableXContractImpl {
    pub fn new() -> Result<Self> {
        let contract_json = fs::read_to_string("dex-contracts/build/contracts/StablecoinConverter.json");
        let address = env::var("STABLEX_CONTRACT_ADDRESS");
        Ok(
            StableXContractImpl {
                base: BaseContract::new(address?, contract_json?)?
            }
        )
    }
}

pub trait StableXContract {
    fn get_current_auction_index(&self) -> Result<U256>;
    fn get_auction_data(&self, _index: U256) -> Result<(AccountState, Vec<Order>)>;

    fn submit_solution(
        &self,
        batch_index: U256,
        owners: Vec<H160>,
        order_ids: Vec<U128>,
        volumes: Vec<U128>,
        prices: Vec<U128>,
        token_ids_for_price: Vec<U128>,
    ) -> Result<()>;
}

impl StableXContract for StableXContractImpl {
    fn get_current_auction_index(&self) -> Result<U256> {
        self.base.contract
            .query("getCurrentStateIndex", (), None, Options::default(), None)
            .wait()
            .map_err(DriverError::from)
    }

    fn get_auction_data(&self, _index: U256) -> Result<(AccountState, Vec<Order>)> {
        unimplemented!();
    }

    fn submit_solution(
        &self,
        batch_index: U256,
        owners: Vec<H160>,
        order_ids: Vec<U128>,
        volumes: Vec<U128>,
        prices: Vec<U128>,
        token_ids_for_price: Vec<U128>,
    ) -> Result<()> {
        let account = self.base
            .account_with_sufficient_balance()
            .ok_or("Not enough balance to send Txs")?;
        self.base.contract
            .call(
                "submitSolution",
               (batch_index, owners, order_ids, volumes, prices, token_ids_for_price),
               account,
               Options::default()
            )
            .wait()
            .map_err(DriverError::from)
            .map(|_| ())
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
        U256,
        Matcher<Vec<H160>>,
        Matcher<Vec<U128>>,
        Matcher<Vec<U128>>,
        Matcher<Vec<U128>>,
        Matcher<Vec<U128>>,
    );

    #[derive(Clone)]
    pub struct StableXContractMock {
        pub get_current_auction_index: Mock<(), Result<U256>>,
        pub get_auction_data: Mock<U256, Result<(AccountState, Vec<Order>)>>,
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
        fn get_auction_data(&self, index: U256) -> Result<(AccountState, Vec<Order>)> {
            self.get_auction_data.called(index)
        }
        fn submit_solution(
            &self,
            batch_index: U256,
            owners: Vec<H160>,
            order_ids: Vec<U128>,
            volumes: Vec<U128>,
            prices: Vec<U128>,
            token_ids_for_price: Vec<U128>,
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
