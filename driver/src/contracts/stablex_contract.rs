// NOTE: Required for automock.
#![cfg_attr(test, allow(clippy::ptr_arg))]

use std::collections::HashMap;
use std::time::Duration;

use anyhow::{anyhow, Result};
use ethcontract::transaction::{GasPrice, ResolveCondition};
use ethcontract::web3::types::Block;
use ethcontract::{Address, BlockNumber, PrivateKey, U256};
use lazy_static::lazy_static;
#[cfg(test)]
use mockall::automock;

use crate::contracts;
use crate::models::{ExecutedOrder, Solution};
use crate::util::FutureWaitExt;
use ethcontract::errors::MethodError;
use ethcontract::transaction::confirm::ConfirmParams;
use ethcontract::web3::futures::Future as _;

lazy_static! {
    // In the BatchExchange smart contract, the objective value will be multiplied by
    // 1 + IMPROVEMENT_DENOMINATOR = 101. Hence, the maximal possible objective value is:
    static ref MAX_OBJECTIVE_VALUE: U256 = U256::max_value() / (U256::from(101));
}

include!(concat!(env!("OUT_DIR"), "/batch_exchange.rs"));
include!(concat!(env!("OUT_DIR"), "/batch_exchange_viewer.rs"));

#[derive(Clone)]
pub struct StableXContractImpl {
    instance: BatchExchange,
    viewer: BatchExchangeViewer,
}

impl StableXContractImpl {
    pub fn new(web3: &contracts::Web3, key: PrivateKey, network_id: u64) -> Result<Self> {
        let defaults = contracts::method_defaults(key, network_id)?;

        let viewer = BatchExchangeViewer::deployed(&web3).wait()?;
        let mut instance = BatchExchange::deployed(&web3).wait()?;
        *instance.defaults_mut() = defaults;

        Ok(StableXContractImpl { instance, viewer })
    }

    pub fn account(&self) -> Address {
        self.instance
            .defaults()
            .from
            .as_ref()
            .map(|from| from.address())
            .unwrap_or_default()
    }

    pub fn address(&self) -> Address {
        self.instance.address()
    }
}

/// Information about an order page that where filtered
/// was applied inside the smart contract.
pub struct FilteredOrderPage {
    pub indexed_elements: Vec<u8>,
    pub has_next_page: bool,
    pub next_page_user: Address,
    pub next_page_user_offset: u16,
}

#[cfg_attr(test, automock)]
pub trait StableXContract {
    /// Retrieve the current batch ID that is accepting orders. Note that this
    /// is always `1` greater than the batch ID that is accepting solutions.
    fn get_current_auction_index(&self) -> Result<u32>;

    /// Retrieve the time remaining in the batch.
    fn get_current_auction_remaining_time(&self) -> Result<Duration>;

    /// Searches for the block number of the last block of the given batch. If
    /// the batch has not yet been finalized, then `None` is returned.
    fn get_last_block_for_batch(&self, batch_id: u32) -> Result<Option<u64>>;

    /// Retrieve one page of indexed auction data that is filtered on chain
    /// to only include orders valid at the given batchId.
    fn get_filtered_auction_data_paginated(
        &self,
        batch_index: U256,
        token_whitelist: Vec<u16>,
        page_size: u16,
        previous_page_user: Address,
        previous_page_user_offset: u16,
        block_number: Option<BlockNumber>,
    ) -> Result<FilteredOrderPage>;

    /// Retrieve one page of auction data.
    /// `block` is needed because the state of the smart contract could change
    /// between blocks which would make the returned auction data inconsistent
    /// between calls.
    fn get_auction_data_paginated(
        &self,
        page_size: u16,
        previous_page_user: Address,
        previous_page_user_offset: u16,
        block_number: Option<BlockNumber>,
    ) -> Result<Vec<u8>>;

    fn get_solution_objective_value(
        &self,
        batch_index: U256,
        solution: Solution,
        block_number: Option<BlockNumber>,
    ) -> Result<U256>;

    fn submit_solution(
        &self,
        batch_index: U256,
        solution: Solution,
        claimed_objective_value: U256,
        gas_price: U256,
        block_timeout: Option<usize>,
    ) -> Result<(), MethodError>;
}

impl StableXContract for StableXContractImpl {
    fn get_current_auction_index(&self) -> Result<u32> {
        let auction_index = self.instance.get_current_batch_id().call().wait()?;
        Ok(auction_index)
    }

    fn get_current_auction_remaining_time(&self) -> Result<Duration> {
        let remaining_seconds = self
            .instance
            .get_seconds_remaining_in_batch()
            .call()
            .wait()?;
        Ok(Duration::from_secs(remaining_seconds.as_u64()))
    }

    fn get_last_block_for_batch(&self, batch_id: u32) -> Result<Option<u64>> {
        let web3 = self.instance.raw_instance().web3();
        let get_block = |block_number: BlockNumber| -> Result<_> {
            web3.eth()
                .block(block_number.into())
                .wait()?
                .ok_or_else(|| anyhow!("block {:?} is missing", block_number))
        };

        let mut current_block = get_block(BlockNumber::Pending)?;
        let mut block_number = None;
        while batch_id < get_block_batch_id(&current_block) {
            if current_block.number == Some(0.into()) {
                // NOTE: We reached the genesis block, this happens with Ganache
                //   tests and means that the current batch being solved started
                //   before the contract was deployed, as such there is no last
                //   block for the solving batch.
                return Ok(None);
            }

            let previous_block_number = current_block.number.ok_or_else(|| {
                anyhow!("block {:?} has a missing block number", current_block.hash)
            })? - 1;
            current_block = get_block(previous_block_number.into())?;
            block_number = Some(previous_block_number.as_u64());
        }

        Ok(block_number)
    }

    fn get_filtered_auction_data_paginated(
        &self,
        batch_index: U256,
        token_whitelist: Vec<u16>,
        page_size: u16,
        previous_page_user: Address,
        previous_page_user_offset: u16,
        block_number: Option<BlockNumber>,
    ) -> Result<FilteredOrderPage> {
        let target_batch = batch_index.low_u32();
        let mut builder = self.viewer.get_filtered_orders_paginated(
            // Balances should be valid for the batch at which we are submitting (target batch + 1)
            [target_batch, target_batch, target_batch + 1],
            token_whitelist,
            previous_page_user,
            previous_page_user_offset,
            page_size,
        );
        builder.block = block_number;
        builder.m.tx.gas = None;
        let (indexed_elements, has_next_page, next_page_user, next_page_user_offset) =
            builder.call().wait()?;
        Ok(FilteredOrderPage {
            indexed_elements,
            has_next_page,
            next_page_user,
            next_page_user_offset,
        })
    }

    fn get_auction_data_paginated(
        &self,
        page_size: u16,
        previous_page_user: Address,
        previous_page_user_offset: u16,
        block_number: Option<BlockNumber>,
    ) -> Result<Vec<u8>> {
        let mut orders_builder = self.viewer.get_encoded_orders_paginated(
            previous_page_user,
            previous_page_user_offset,
            U256::from(page_size),
        );
        orders_builder.block = block_number.or(Some(BlockNumber::Pending));
        orders_builder.m.tx.gas = None;
        orders_builder.call().wait().map_err(From::from)
    }

    fn get_solution_objective_value(
        &self,
        batch_index: U256,
        solution: Solution,
        block_number: Option<BlockNumber>,
    ) -> Result<U256> {
        let (prices, token_ids_for_price) = encode_prices_for_contract(&solution.prices);
        let (owners, order_ids, volumes) = encode_execution_for_contract(&solution.executed_orders);
        let mut builder = self
            .instance
            .submit_solution(
                batch_index.low_u32(),
                *MAX_OBJECTIVE_VALUE,
                owners,
                order_ids,
                volumes,
                prices,
                token_ids_for_price,
            )
            .view();
        builder.block = block_number;
        let objective_value = builder.call().wait()?;
        Ok(objective_value)
    }

    fn submit_solution(
        &self,
        batch_index: U256,
        solution: Solution,
        claimed_objective_value: U256,
        gas_price: U256,
        block_timeout: Option<usize>,
    ) -> Result<(), MethodError> {
        let (prices, token_ids_for_price) = encode_prices_for_contract(&solution.prices);
        let (owners, order_ids, volumes) = encode_execution_for_contract(&solution.executed_orders);
        let mut method = self
            .instance
            .submit_solution(
                batch_index.low_u32(),
                claimed_objective_value,
                owners,
                order_ids,
                volumes,
                prices,
                token_ids_for_price,
            )
            .gas_price(GasPrice::Value(gas_price))
            // NOTE: Gas estimate might be off, as we race with other solution
            //   submissions and thus might have to revert trades which costs
            //   more gas than expected.
            .gas(5_500_000.into());

        method.tx.resolve = Some(ResolveCondition::Confirmed(ConfirmParams {
            block_timeout,
            ..Default::default()
        }));
        method.send().wait()?;

        Ok(())
    }
}

fn encode_prices_for_contract(price_map: &HashMap<u16, u128>) -> (Vec<u128>, Vec<u16>) {
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
        .map(|token_id| price_map[token_id])
        .collect();
    (prices, token_ids)
}

fn encode_execution_for_contract(
    executed_orders: &[ExecutedOrder],
) -> (Vec<Address>, Vec<u16>, Vec<u128>) {
    let mut owners = vec![];
    let mut order_ids = vec![];
    let mut volumes = vec![];
    for order in executed_orders {
        if order.buy_amount > 0 {
            // order was touched!
            // Note that above condition is only holds for sell orders.
            owners.push(order.account_id);
            order_ids.push(order.order_id);
            volumes.push(order.buy_amount);
        }
    }
    (owners, order_ids, volumes)
}

fn get_block_batch_id<T>(block: &Block<T>) -> u32 {
    const BATCH_DURATION: u64 = 300;
    (block.timestamp.as_u64() / BATCH_DURATION) as _
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use crate::util::test_util::map_from_slice;

    #[test]
    fn generic_encode_execution_test() {
        let address_1 = Address::from_low_u64_be(1);
        let address_2 = Address::from_low_u64_be(2);

        let order_1 = ExecutedOrder {
            order_id: 0,
            account_id: address_1,
            sell_amount: 1,
            buy_amount: 1,
        };
        let order_2 = ExecutedOrder {
            order_id: 1,
            account_id: address_2,
            sell_amount: 0,
            buy_amount: 0,
        };

        let expected_owners = vec![address_1];
        let expected_order_ids = vec![0];
        let expected_volumes = vec![1];

        let expected_results = (expected_owners, expected_order_ids, expected_volumes);

        assert_eq!(
            encode_execution_for_contract(&[order_1, order_2]),
            expected_results
        );
    }

    #[test]
    fn generic_price_encoding() {
        let price_map = map_from_slice(&[(0, u128::max_value()), (1, 0), (2, 1), (3, 2)]);
        // Only contain non fee-tokens and non zero prices
        let expected_prices = vec![1, 2];
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
        let expected_prices = vec![3, 1, 2];
        let expected_token_ids = vec![1, 3, 4];
        assert_eq!(
            encode_prices_for_contract(&unordered_price_map),
            (expected_prices, expected_token_ids)
        );
    }
}
