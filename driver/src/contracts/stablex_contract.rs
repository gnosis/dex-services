#[cfg(test)]
extern crate mock_it;

use dfusion_core::models::{AccountState, Order, Solution};

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
    base: BaseContract,
}

#[allow(dead_code)] // TODO - remove once used
impl StableXContractImpl {
    pub fn new() -> Result<Self> {
        let contract_json =
            fs::read_to_string("dex-contracts/build/contracts/StablecoinConverter.json")?;
        let address = env::var("STABLEX_CONTRACT_ADDRESS")?;
        Ok(StableXContractImpl {
            base: BaseContract::new(address, contract_json)?,
        })
    }
}

pub trait StableXContract {
    fn get_current_auction_index(&self) -> Result<U256>;
    fn get_auction_data(&self, _index: U256) -> Result<(AccountState, Vec<Order>)>;
    fn submit_solution(
        &self,
        batch_index: U256,
        orders: Vec<Order>,
        solution: Solution,
    ) -> Result<()>;
}

impl StableXContract for StableXContractImpl {
    fn get_current_auction_index(&self) -> Result<U256> {
        self.base
            .contract
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
        orders: Vec<Order>,
        solution: Solution,
    ) -> Result<()> {
        let account = self
            .base
            .account_with_sufficient_balance()
            .ok_or("Not enough balance to send Txs")?;

        let (owners, order_ids, volumes, prices, token_ids_for_price) =
            parse_auction_results(orders, solution);

        self.base
            .contract
            .call(
                "submitSolution",
                (
                    batch_index,
                    owners,
                    order_ids,
                    volumes,
                    prices,
                    token_ids_for_price,
                ),
                account,
                Options::default(),
            )
            .wait()
            .map_err(DriverError::from)
            .map(|_| ())
    }
}

type ContractRecognizedAuctionResults = (Vec<H160>, Vec<U128>, Vec<U128>, Vec<U128>, Vec<U128>);

fn parse_auction_results(
    orders: Vec<Order>,
    solution: Solution,
) -> ContractRecognizedAuctionResults {
    // Representing the solution's price vector more compactly as:
    // sorted_touched_token_ids, non_zero_prices which are logically bound by index.
    // Example solution.prices = [3, 0, 1] will be transformed into [0, 2], [3, 1]
    let mut ordered_token_ids: Vec<U128> = vec![];
    let mut prices: Vec<U128> = vec![];
    for (token_id, price) in solution.prices.into_iter().enumerate() {
        if price > 0 {
            ordered_token_ids.push(U128::from(token_id as usize));
            prices.push(U128::from(price as usize));
        }
    }

    let mut owners: Vec<H160> = vec![];
    let mut order_ids: Vec<U128> = vec![];
    let mut volumes: Vec<U128> = vec![];
    let zipped_amounts = solution
        .executed_buy_amounts
        .into_iter()
        .zip(solution.executed_sell_amounts.into_iter());
    for (order_id, (buy_amount, sell_amount)) in zipped_amounts.enumerate() {
        if buy_amount > 0 && sell_amount > 0 {
            // order was touched!
            owners.push(orders[order_id].account_id);
            order_ids.push(U128::from(order_id));
            // Currently all orders are sell orders, so volumes are sell_amounts.
            volumes.push(U128::from(sell_amount as usize));
        }
    }
    (owners, order_ids, volumes, prices, ordered_token_ids)
}

#[cfg(test)]
pub mod tests {
    use mock_it::Matcher;
    use mock_it::Matcher::*;
    use mock_it::Mock;

    use crate::error::ErrorKind;

    use super::*;

    type SubmitSolutionArguments = (U256, Matcher<Vec<Order>>, Matcher<Solution>);

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
            orders: Vec<Order>,
            solution: Solution,
        ) -> Result<()> {
            self.submit_solution
                .called((batch_index, Val(orders), Val(solution)))
        }
    }

    #[test]
    fn test_parse_auction_results() {
        let solution = Solution {
            surplus: None,
            prices: vec![3, 0, 1],
            executed_sell_amounts: vec![1, 3],
            executed_buy_amounts: vec![3, 1],
        };

        let address_1 = H160::from(1);
        let address_2 = H160::from(0);

        let order_1 = Order {
            batch_information: None,
            account_id: address_1,
            sell_token: 0,
            buy_token: 2,
            sell_amount: 1,
            buy_amount: 2,
        };
        let order_2 = Order {
            batch_information: None,
            account_id: address_2,
            sell_token: 2,
            buy_token: 0,
            sell_amount: 3,
            buy_amount: 4,
        };

        let zero = U128::from(0);
        let one = U128::from(1);
        let two = U128::from(2);
        let three = U128::from(3);

        let expected_owners = vec![address_1, address_2];
        let expected_order_ids = vec![zero, one];
        let expected_volumes = vec![one, three];
        let expected_prices = vec![three, one];
        let expected_token_ids = vec![zero, two];

        let expected_results = (
            expected_owners,
            expected_order_ids,
            expected_volumes,
            expected_prices,
            expected_token_ids,
        );

        assert_eq!(
            parse_auction_results(vec![order_1, order_2], solution),
            expected_results
        );
    }
}
