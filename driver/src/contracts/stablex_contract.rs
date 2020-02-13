#![allow(clippy::ptr_arg)] // required for automock

use std::collections::HashMap;
use std::env;

use ethcontract::transaction::GasPrice;
use ethcontract::DynWeb3;
use lazy_static::lazy_static;
#[cfg(test)]
use mockall::automock;
use web3::types::{H160, U128, U256};

use crate::contracts;
use crate::error::DriverError;
use crate::models::{Order, Solution};
use crate::util::FutureWaitExt;

type Result<T> = std::result::Result<T, DriverError>;

lazy_static! {
    // In the BatchExchange smart contract, the objective value will be multiplied by
    // 1 + IMPROVEMENT_DENOMINATOR = 101. Hence, the maximal possible objective value is:
    static ref MAX_OBJECTIVE_VALUE: U256 = U256::max_value() / (U256::from(101));
}

include!(concat!(env!("OUT_DIR"), "/batch_exchange.rs"));

impl BatchExchange {
    pub fn new(web3: &contracts::Web3, network_id: u64) -> Result<Self> {
        let defaults = contracts::method_defaults(network_id)?;

        let mut instance = BatchExchange::deployed(&web3).wait()?;
        *instance.defaults_mut() = defaults;

        Ok(instance)
    }

    pub fn account(&self) -> H160 {
        self.defaults()
            .from
            .as_ref()
            .map(|from| from.address())
            .unwrap_or_default()
    }

    pub fn web3(&self) -> DynWeb3 {
        self.instance.web3()
    }
}

impl Clone for BatchExchange {
    fn clone(&self) -> Self {
        BatchExchange {
            instance: self.instance.clone(),
        }
    }
}

#[cfg_attr(test, automock)]
pub trait StableXContract {
    /// Retrieves the current number of registered tokens in the contract.
    fn get_num_tokens(&self) -> Result<u16>;

    fn get_current_auction_index(&self) -> Result<U256>;

    /// Retrieve one page of auction data.
    /// `block` is needed because the state of the smart contract could change
    /// between blocks which would make the returned auction data inconsistent
    /// between calls.
    fn get_auction_data_paginated(
        &self,
        block: u64,
        page_size: u64,
        previous_page_user: H160,
        previous_page_user_offset: u64,
    ) -> Result<Vec<u8>>;

    fn get_solution_objective_value(
        &self,
        batch_index: U256,
        orders: Vec<Order>,
        solution: Solution,
    ) -> Result<U256>;

    fn submit_solution(
        &self,
        batch_index: U256,
        orders: Vec<Order>,
        solution: Solution,
        claimed_objective_value: U256,
    ) -> Result<()>;
}

impl StableXContract for BatchExchange {
    fn get_num_tokens(&self) -> Result<u16> {
        let num_tokens = self.num_tokens().call().wait()?;
        Ok(num_tokens as _)
    }

    fn get_current_auction_index(&self) -> Result<U256> {
        let auction_index = self.get_current_batch_id().call().wait()?;
        Ok(auction_index.into())
    }

    fn get_auction_data_paginated(
        &self,
        block: u64,
        page_size: u64,
        previous_page_user: H160,
        previous_page_user_offset: u64,
    ) -> Result<Vec<u8>> {
        let mut orders_builder = self.get_encoded_users_paginated(
            previous_page_user,
            previous_page_user_offset,
            page_size,
        );
        orders_builder.m.tx.gas = None;
        orders_builder.block = Some(web3::types::BlockNumber::Number(block));
        orders_builder.call().wait().map_err(From::from)
    }

    fn get_solution_objective_value(
        &self,
        batch_index: U256,
        orders: Vec<Order>,
        solution: Solution,
    ) -> Result<U256> {
        let (prices, token_ids_for_price) = encode_prices_for_contract(&solution.prices);
        let (owners, order_ids, volumes) =
            encode_execution_for_contract(orders, solution.executed_buy_amounts);
        let objective_value = self
            .submit_solution(
                batch_index.low_u64(),
                *MAX_OBJECTIVE_VALUE,
                owners,
                order_ids,
                volumes,
                prices,
                token_ids_for_price,
            )
            .call()
            .wait()?;

        Ok(objective_value)
    }

    fn submit_solution(
        &self,
        batch_index: U256,
        orders: Vec<Order>,
        solution: Solution,
        claimed_objective_value: U256,
    ) -> Result<()> {
        let (prices, token_ids_for_price) = encode_prices_for_contract(&solution.prices);
        let (owners, order_ids, volumes) =
            encode_execution_for_contract(orders, solution.executed_buy_amounts);
        self.submit_solution(
            batch_index.low_u64(),
            claimed_objective_value,
            owners,
            order_ids,
            volumes,
            prices,
            token_ids_for_price,
        )
        .gas_price(GasPrice::Scaled(3.0))
        .send()
        .wait()?;

        Ok(())
    }
}

fn encode_prices_for_contract(price_map: &HashMap<u16, u128>) -> (Vec<U128>, Vec<u64>) {
    // Representing the solution's price vector as:
    // sorted_touched_token_ids, non_zero_prices (excluding price at token with id 0)
    let mut token_ids: Vec<u16> = price_map
        .keys()
        .copied()
        .filter(|t| *t > 0 && price_map[t] > 0)
        .collect();
    token_ids.sort_unstable();
    let prices = token_ids
        .iter()
        .map(|token_id| U128::from(price_map[token_id]))
        .collect();
    (prices, token_ids.iter().map(|t| *t as u64).collect())
}

fn encode_execution_for_contract(
    orders: Vec<Order>,
    executed_buy_amounts: Vec<u128>,
) -> (Vec<H160>, Vec<u64>, Vec<U128>) {
    assert_eq!(
        orders.len(),
        executed_buy_amounts.len(),
        "Received inconsistent auction result data."
    );
    let mut owners = vec![];
    let mut order_ids = vec![];
    let mut volumes = vec![];
    for (order_index, buy_amount) in executed_buy_amounts.into_iter().enumerate() {
        if buy_amount > 0 {
            // order was touched!
            // Note that above condition is only holds for sell orders.
            owners.push(orders[order_index].account_id);
            order_ids.push(orders[order_index].id as _);
            volumes.push(U128::from(buy_amount.to_be_bytes()));
        }
    }
    (owners, order_ids, volumes)
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use crate::util::test_util::map_from_slice;

    #[test]
    #[should_panic]
    fn encode_execution_fails_on_inconsistent_results() {
        let some_reasonable_order = Order {
            id: 0,
            account_id: H160::from_low_u64_be(1),
            sell_token: 0,
            buy_token: 1,
            sell_amount: 1,
            buy_amount: 1,
        };
        encode_execution_for_contract(vec![some_reasonable_order], vec![]);
    }

    #[test]
    fn generic_encode_execution_test() {
        let executed_buy_amounts = vec![1, 0];

        let address_1 = H160::from_low_u64_be(1);
        let address_2 = H160::from_low_u64_be(2);

        let order_1 = Order {
            id: 0,
            account_id: address_1,
            sell_token: 0,
            buy_token: 2,
            sell_amount: 1,
            buy_amount: 2,
        };
        let order_2 = Order {
            id: 1,
            account_id: address_2,
            sell_token: 2,
            buy_token: 0,
            sell_amount: 3,
            buy_amount: 4,
        };

        let expected_owners = vec![address_1];
        let expected_order_ids = vec![0];
        let expected_volumes = vec![U128::from(1)];

        let expected_results = (expected_owners, expected_order_ids, expected_volumes);

        assert_eq!(
            encode_execution_for_contract(vec![order_1, order_2], executed_buy_amounts),
            expected_results
        );
    }

    #[test]
    fn generic_price_encoding() {
        let price_map = map_from_slice(&[(0, u128::max_value()), (1, 0), (2, 1), (3, 2)]);
        // Only contain non fee-tokens and non zero prices
        let expected_prices = vec![1.into(), 2.into()];
        let expected_token_ids = vec![2, 3];

        assert_eq!(
            encode_prices_for_contract(&price_map),
            (expected_prices, expected_token_ids)
        );
    }

    #[test]
    fn unsorted_price_encoding() {
        let unordered_price_map = map_from_slice(&[(4, 2), (1, 3), (5, 0), (0, 2), (3, 1)]);

        // Only contain non fee-token and non zero prices
        let expected_prices = vec![3.into(), 1.into(), 2.into()];
        let expected_token_ids = vec![1u64, 3u64, 4u64];
        assert_eq!(
            encode_prices_for_contract(&unordered_price_map),
            (expected_prices, expected_token_ids)
        );
    }
}
