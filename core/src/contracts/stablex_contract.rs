// NOTE: Required for automock.
#![cfg_attr(test, allow(clippy::ptr_arg))]

mod search_batches;

use crate::{
    contracts,
    models::{ExecutedOrder, Solution},
};
use anyhow::{anyhow, Error, Result};
use ethcontract::{
    contract::Event,
    errors::{ExecutionError, MethodError},
    transaction::{confirm::ConfirmParams, Account, GasPrice, ResolveCondition, TransactionResult},
    Address, BlockNumber, PrivateKey, U256,
};
use futures::{
    future::{BoxFuture, FutureExt as _},
    stream::{BoxStream, StreamExt as _},
};
use lazy_static::lazy_static;
#[cfg(test)]
use mockall::automock;
use std::collections::HashMap;
use std::time::Duration;

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
    pub async fn new(web3: &contracts::Web3, key: PrivateKey, network_id: u64) -> Result<Self> {
        let defaults = contracts::method_defaults(key, network_id)?;

        let viewer = BatchExchangeViewer::deployed(&web3).await?;
        let mut instance = BatchExchange::deployed(&web3).await?;
        *instance.defaults_mut() = defaults;

        Ok(StableXContractImpl { instance, viewer })
    }

    pub fn account(&self) -> Option<Account> {
        self.instance.defaults().from.clone()
    }

    pub fn address(&self) -> Address {
        self.instance.address()
    }

    pub async fn num_tokens(&self) -> Result<u16> {
        self.instance.num_tokens().call().await.map_err(Error::from)
    }

    pub async fn get_token_info(&self, id: u16) -> Result<(Address, String, u8)> {
        self.viewer
            .get_token_info(id)
            .call()
            .await
            .map_err(Error::from)
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

#[derive(thiserror::Error, Debug)]
pub enum NoopTransactionError {
    #[error("no account")]
    NoAccount,
    #[error("execution error: {0}")]
    ExecutedOrder(#[from] ExecutionError),
}

#[cfg_attr(test, automock)]
pub trait StableXContract {
    /// Retrieve the current batch ID that is accepting orders. Note that this
    /// is always `1` greater than the batch ID that is accepting solutions.
    fn get_current_auction_index<'a>(&'a self) -> BoxFuture<'a, Result<u32>>;

    /// Retrieve the time remaining in the batch.
    fn get_current_auction_remaining_time<'a>(&'a self) -> BoxFuture<'a, Result<Duration>>;

    /// Searches for the block number of the last block of the given batch. If
    /// the batch has not yet been finalized, then the block number for the
    /// `"latest"` block is returned.
    fn get_last_block_for_batch<'a>(&'a self, batch_id: u32) -> BoxFuture<'a, Result<u64>>;

    /// Retrieve one page of indexed auction data that is filtered on chain
    /// to only include orders valid at the given batchId.
    fn get_filtered_auction_data_paginated<'a>(
        &'a self,
        batch_index: u32,
        token_whitelist: Vec<u16>,
        page_size: u16,
        previous_page_user: Address,
        previous_page_user_offset: u16,
        block_number: Option<BlockNumber>,
    ) -> BoxFuture<'a, Result<FilteredOrderPage>>;

    /// Retrieve one page of auction data.
    /// `block` is needed because the state of the smart contract could change
    /// between blocks which would make the returned auction data inconsistent
    /// between calls.
    fn get_auction_data_paginated<'a>(
        &'a self,
        page_size: u16,
        previous_page_user: Address,
        previous_page_user_offset: u16,
        block_number: Option<BlockNumber>,
    ) -> BoxFuture<'a, Result<Vec<u8>>>;

    fn get_solution_objective_value<'a>(
        &'a self,
        batch_index: u32,
        solution: Solution,
        block_number: Option<BlockNumber>,
    ) -> BoxFuture<'a, Result<U256>>;

    fn submit_solution<'a>(
        &'a self,
        batch_index: u32,
        solution: Solution,
        claimed_objective_value: U256,
        gas_price: U256,
        block_timeout: Option<usize>,
        nonce: U256,
    ) -> BoxFuture<'a, Result<(), MethodError>>;

    fn past_events<'a>(
        &'a self,
        from_block: BlockNumber,
        to_block: BlockNumber,
    ) -> BoxFuture<'a, Result<Vec<Event<batch_exchange::Event>>, ExecutionError>>;

    fn stream_events<'a>(
        &'a self,
    ) -> BoxStream<'a, Result<Event<batch_exchange::Event>, ExecutionError>>;

    /// Create a noop transaction. Useful to cancel a previous transaction that is stuck due to
    /// low gas price.
    fn send_noop_transaction<'a>(
        &'a self,
        gas_price: U256,
        nonce: U256,
    ) -> BoxFuture<'a, Result<TransactionResult, NoopTransactionError>>;

    /// The current nonce aka transaction_count.
    fn get_transaction_count<'a>(&'a self) -> BoxFuture<'a, Result<U256>>;
}

impl StableXContract for StableXContractImpl {
    fn get_current_auction_index<'a>(&'a self) -> BoxFuture<'a, Result<u32>> {
        async move {
            self.instance
                .get_current_batch_id()
                .call()
                .await
                .map_err(Error::from)
        }
        .boxed()
    }

    fn get_current_auction_remaining_time<'a>(&'a self) -> BoxFuture<'a, Result<Duration>> {
        async move {
            let seconds = self
                .instance
                .get_seconds_remaining_in_batch()
                .call()
                .await?;
            Ok(Duration::from_secs(seconds.as_u64()))
        }
        .boxed()
    }

    fn get_last_block_for_batch<'a>(&'a self, batch_id: u32) -> BoxFuture<'a, Result<u64>> {
        async move {
            let web3 = self.instance.raw_instance().web3();
            search_batches::search_last_block_for_batch(&web3, batch_id).await
        }
        .boxed()
    }

    fn get_filtered_auction_data_paginated<'a>(
        &'a self,
        batch_index: u32,
        token_whitelist: Vec<u16>,
        page_size: u16,
        previous_page_user: Address,
        previous_page_user_offset: u16,
        block_number: Option<BlockNumber>,
    ) -> BoxFuture<'a, Result<FilteredOrderPage>> {
        async move {
            let target_batch = batch_index;
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
            let future = builder.call();
            let (indexed_elements, has_next_page, next_page_user, next_page_user_offset) =
                future.await?;
            Ok(FilteredOrderPage {
                indexed_elements,
                has_next_page,
                next_page_user,
                next_page_user_offset,
            })
        }
        .boxed()
    }

    fn get_auction_data_paginated<'a>(
        &'a self,
        page_size: u16,
        previous_page_user: Address,
        previous_page_user_offset: u16,
        block_number: Option<BlockNumber>,
    ) -> BoxFuture<'a, Result<Vec<u8>>> {
        async move {
            let mut orders_builder = self.viewer.get_encoded_orders_paginated(
                previous_page_user,
                previous_page_user_offset,
                U256::from(page_size),
            );
            orders_builder.block = block_number;
            orders_builder.m.tx.gas = None;
            orders_builder.call().await.map_err(Error::from)
        }
        .boxed()
    }

    fn get_solution_objective_value<'a>(
        &'a self,
        batch_index: u32,
        solution: Solution,
        block_number: Option<BlockNumber>,
    ) -> BoxFuture<'a, Result<U256>> {
        async move {
            let (prices, token_ids_for_price) = encode_prices_for_contract(&solution.prices);
            let (owners, order_ids, volumes) =
                encode_execution_for_contract(&solution.executed_orders);
            let mut builder = self
                .instance
                .submit_solution(
                    batch_index,
                    *MAX_OBJECTIVE_VALUE,
                    owners,
                    order_ids,
                    volumes,
                    prices,
                    token_ids_for_price,
                )
                .view();
            builder.block = block_number;
            builder.call().await.map_err(Error::from)
        }
        .boxed()
    }

    fn submit_solution<'a>(
        &'a self,
        batch_index: u32,
        solution: Solution,
        claimed_objective_value: U256,
        gas_price: U256,
        block_timeout: Option<usize>,
        nonce: U256,
    ) -> BoxFuture<'a, Result<(), MethodError>> {
        async move {
            let (prices, token_ids_for_price) = encode_prices_for_contract(&solution.prices);
            let (owners, order_ids, volumes) =
                encode_execution_for_contract(&solution.executed_orders);
            let mut method = self
                .instance
                .submit_solution(
                    batch_index,
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
                .gas(5_500_000.into())
                .nonce(nonce);

            method.tx.resolve = Some(ResolveCondition::Confirmed(ConfirmParams {
                block_timeout,
                ..Default::default()
            }));
            method.send().await.map(|_| ())
        }
        .boxed()
    }

    fn past_events<'a>(
        &'a self,
        from_block: BlockNumber,
        to_block: BlockNumber,
    ) -> BoxFuture<'a, Result<Vec<Event<batch_exchange::Event>>, ExecutionError>> {
        self.instance
            .all_events()
            .from_block(from_block)
            .to_block(to_block)
            .query_past_events_paginated()
            .boxed()
    }

    fn stream_events<'a>(
        &'a self,
    ) -> BoxStream<'a, Result<Event<batch_exchange::Event>, ExecutionError>> {
        self.instance
            .all_events()
            .from_block(BlockNumber::Latest)
            .stream()
            .boxed()
    }

    fn send_noop_transaction<'a>(
        &'a self,
        gas_price: U256,
        nonce: U256,
    ) -> BoxFuture<'a, Result<TransactionResult, NoopTransactionError>> {
        async move {
            let web3 = self.instance.raw_instance().web3();
            let account = self.account().ok_or(NoopTransactionError::NoAccount)?;
            let address = account.address();
            let transaction = ethcontract::transaction::TransactionBuilder::new(web3)
                .from(account)
                .to(address)
                .gas(U256::from(21000))
                .gas_price(GasPrice::Value(gas_price))
                .nonce(nonce)
                .value(U256::zero())
                .resolve(ResolveCondition::Confirmed(ConfirmParams::mined()));
            transaction.send().await.map_err(From::from)
        }
        .boxed()
    }

    fn get_transaction_count<'a>(&'a self) -> BoxFuture<'a, Result<U256>> {
        use futures::compat::Future01CompatExt as _;
        async move {
            let web3 = self.instance.raw_instance().web3();
            let account = self.account().ok_or_else(|| anyhow!("no account"))?;
            let address = account.address();
            web3.eth()
                .transaction_count(address, None)
                .compat()
                .await
                .map_err(From::from)
        }
        .boxed()
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
