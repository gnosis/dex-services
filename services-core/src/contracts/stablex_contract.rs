// NOTE: Required for automock.
#![cfg_attr(test, allow(clippy::ptr_arg))]

mod search_batches;

use crate::{
    contracts,
    models::{ExecutedOrder, Solution},
};
use ::contracts::{batch_exchange, BatchExchange, BatchExchangeViewer};
use anyhow::{anyhow, Error, Result};
use ethcontract::{
    contract::Event,
    errors::{ExecutionError, MethodError},
    transaction::{confirm::ConfirmParams, Account, GasPrice, ResolveCondition, TransactionResult},
    Address, BlockId, BlockNumber, PrivateKey, U256,
};
use futures::future::{BoxFuture, FutureExt as _};

use lazy_static::lazy_static;
use std::collections::HashMap;
use std::time::Duration;

lazy_static! {
    // In the BatchExchange smart contract, the objective value will be multiplied by
    // 1 + IMPROVEMENT_DENOMINATOR = 101. Hence, the maximal possible objective value is:
    static ref MAX_OBJECTIVE_VALUE: U256 = U256::max_value() / (U256::from(101));
}

#[derive(Clone)]
pub struct StableXContractImpl {
    instance: BatchExchange,
    viewer: BatchExchangeViewer,
}

impl StableXContractImpl {
    pub async fn new(web3: &contracts::Web3, key: PrivateKey) -> Result<Self> {
        let chain_id = web3.eth().chain_id().await?.as_u64();
        let defaults = contracts::method_defaults(key, chain_id)?;

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
    ExecutionError(#[from] ExecutionError),
}

#[cfg_attr(test, mockall::automock)]
#[async_trait::async_trait]
pub trait StableXContract: Send + Sync {
    /// Retrieve the current batch ID that is accepting orders. Note that this
    /// is always `1` greater than the batch ID that is accepting solutions.
    async fn get_current_auction_index(&self) -> Result<u32>;

    /// Retrieve the time remaining in the batch.
    async fn get_current_auction_remaining_time(&self) -> Result<Duration>;

    /// Searches for the block number of the last block of the given batch. If
    /// the batch has not yet been finalized, then the block number for the
    /// `"latest"` block is returned.
    async fn get_last_block_for_batch(&self, batch_id: u32) -> Result<u64>;

    /// Retrieve one page of indexed auction data that is filtered on chain
    /// to only include orders valid at the given batchId.
    async fn get_filtered_auction_data_paginated(
        &self,
        batch_index: u32,
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
    async fn get_auction_data_paginated(
        &self,
        page_size: u16,
        previous_page_user: Address,
        previous_page_user_offset: u16,
        block_number: Option<BlockNumber>,
    ) -> Result<Vec<u8>>;

    async fn get_solution_objective_value(
        &self,
        batch_index: u32,
        solution: Solution,
        block_number: Option<BlockNumber>,
    ) -> Result<U256>;

    fn submit_solution<'a>(
        &'a self,
        batch_index: u32,
        solution: Solution,
        claimed_objective_value: U256,
        gas_price: U256,
        nonce: U256,
        gas_limit: U256,
    ) -> BoxFuture<'a, Result<(), MethodError>>;

    async fn past_events(
        &self,
        from_block: BlockNumber,
        to_block: BlockNumber,
        block_page_size: u64,
    ) -> Result<Vec<Event<batch_exchange::Event>>, ExecutionError>;

    /// Create a noop transaction. Useful to cancel a previous transaction that is stuck due to
    /// low gas price.
    fn send_noop_transaction<'a>(
        &'a self,
        gas_price: U256,
        nonce: U256,
    ) -> BoxFuture<'a, Result<TransactionResult, NoopTransactionError>>;

    /// The current nonce aka transaction_count.
    async fn get_transaction_count(&self) -> Result<U256>;
}

#[async_trait::async_trait]
impl StableXContract for StableXContractImpl {
    async fn get_current_auction_index(&self) -> Result<u32> {
        self.instance
            .get_current_batch_id()
            .call()
            .await
            .map_err(Error::from)
    }

    async fn get_current_auction_remaining_time(&self) -> Result<Duration> {
        let seconds = self
            .instance
            .get_seconds_remaining_in_batch()
            .call()
            .await?;
        Ok(Duration::from_secs(seconds.as_u64()))
    }

    async fn get_last_block_for_batch(&self, batch_id: u32) -> Result<u64> {
        let web3 = self.instance.raw_instance().web3();
        search_batches::search_last_block_for_batch(&web3, batch_id).await
    }

    async fn get_filtered_auction_data_paginated(
        &self,
        batch_index: u32,
        token_whitelist: Vec<u16>,
        page_size: u16,
        previous_page_user: Address,
        previous_page_user_offset: u16,
        block_number: Option<BlockNumber>,
    ) -> Result<FilteredOrderPage> {
        let target_batch = batch_index;
        let mut builder = self.viewer.get_filtered_orders_paginated(
            // Balances should be valid for the batch at which we are submitting (target batch + 1)
            [target_batch, target_batch, target_batch + 1],
            token_whitelist,
            previous_page_user,
            previous_page_user_offset,
            page_size,
        );
        builder.block = block_number.map(BlockId::Number);
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

    async fn get_auction_data_paginated(
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
        orders_builder.block = block_number.map(BlockId::Number);
        orders_builder.m.tx.gas = None;
        orders_builder.call().await.map_err(Error::from)
    }

    async fn get_solution_objective_value(
        &self,
        batch_index: u32,
        solution: Solution,
        block_number: Option<BlockNumber>,
    ) -> Result<U256> {
        let (prices, token_ids_for_price) = encode_prices_for_contract(&solution.prices);
        let (owners, order_ids, volumes) = encode_execution_for_contract(&solution.executed_orders);
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
        builder.block = block_number.map(BlockId::Number);
        builder.call().await.map_err(Error::from)
    }

    fn submit_solution<'a>(
        &'a self,
        batch_index: u32,
        solution: Solution,
        claimed_objective_value: U256,
        gas_price: U256,
        nonce: U256,
        gas_limit: U256,
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
                .gas(gas_limit)
                .nonce(nonce);
            method.tx.resolve = Some(ResolveCondition::Confirmed(ConfirmParams::mined()));
            method.send().await.map(|_| ())
        }
        .boxed()
    }

    async fn past_events(
        &self,
        from_block: BlockNumber,
        to_block: BlockNumber,
        block_page_size: u64,
    ) -> Result<Vec<Event<batch_exchange::Event>>, ExecutionError> {
        self.instance
            .all_events()
            .from_block(from_block)
            .to_block(to_block)
            .block_page_size(block_page_size)
            .query_paginated()
            .await
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

    async fn get_transaction_count(&self) -> Result<U256> {
        let web3 = self.instance.raw_instance().web3();
        let account = self.account().ok_or_else(|| anyhow!("no account"))?;
        let address = account.address();
        web3.eth()
            .transaction_count(address, None)
            .await
            .map_err(From::from)
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
