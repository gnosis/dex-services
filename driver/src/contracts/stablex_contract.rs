#![allow(clippy::ptr_arg)] // required for automock

use std::collections::HashMap;
use std::env;

use dfusion_core::models::{AccountState, BatchInformation, Order, Solution};
use lazy_static::lazy_static;
#[cfg(test)]
use mockall::automock;
use web3::transports::EventLoopHandle;
use web3::types::{H160, U128, U256};

use crate::contracts;
use crate::contracts::stablex_auction_element::StableXAuctionElement;
use crate::error::DriverError;
use crate::util::FutureWaitExt;

type Result<T> = std::result::Result<T, DriverError>;

pub const AUCTION_ELEMENT_WIDTH: usize = 112;

lazy_static! {
    // In the BatchExchange smart contract, the objective value will be multiplied by
    // 1 + IMPROVEMENT_DENOMINATOR = 101. Hence, the maximal possible objective value is:
    static ref MAX_OBJECTIVE_VALUE: U256 = U256::max_value() / (U256::from(101));
}

include!(concat!(env!("OUT_DIR"), "/batch_exchange.rs"));

impl BatchExchange {
    pub fn new() -> Result<(Self, EventLoopHandle)> {
        let (web3, event_loop) = contracts::web3_provider()?;
        let defaults = contracts::method_defaults()?;

        let mut instance = BatchExchange::deployed(&web3).wait()?;
        *instance.defaults_mut() = defaults;

        Ok((instance, event_loop))
    }

    pub fn account(&self) -> H160 {
        self.defaults()
            .from
            .as_ref()
            .map(|from| from.address())
            .unwrap_or_default()
    }
}

#[cfg_attr(test, automock)]
pub trait StableXContract {
    fn get_current_auction_index(&self) -> Result<U256>;
    fn get_auction_data(&self, index: U256) -> Result<(AccountState, Vec<Order>)>;
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
    fn get_current_auction_index(&self) -> Result<U256> {
        let auction_index = self.get_current_batch_id().call().wait()?;
        Ok(auction_index.into())
    }

    fn get_auction_data(&self, index: U256) -> Result<(AccountState, Vec<Order>)> {
        let mut orders_builder = self.get_encoded_orders();
        // NOTE: we need to override the gas limit which was set by the method
        //   defaults - large number of orders was causing this `eth_call`
        //   request to run into the gas limit
        orders_builder.m.tx.gas = None;
        let packed_auction_bytes = orders_builder.call().wait()?;
        Ok(get_auction_data(&packed_auction_bytes, index))
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
            .gas(5_000_000.into())
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
        .gas(5_000_000.into())
        .gas_price(20_000_000_000u64.into())
        .send()
        .wait()?;

        Ok(())
    }
}

fn get_auction_data(packed_auction_bytes: &[u8], index: U256) -> (AccountState, Vec<Order>) {
    let auction_elements = parse_auction_elements(packed_auction_bytes, index, &mut HashMap::new());
    let account_state = auction_elements_to_account_state(auction_elements.iter());
    let orders = auction_elements
        .into_iter()
        .map(|auction_element| auction_element.order)
        .collect();
    (account_state, orders)
}

fn parse_auction_elements(
    packed_auction_bytes: &[u8],
    index: U256,
    user_order_counts: &mut HashMap<H160, u16>,
) -> Vec<StableXAuctionElement> {
    assert_eq!(
        packed_auction_bytes.len() % AUCTION_ELEMENT_WIDTH,
        0,
        "Each auction should be packed in {} bytes",
        AUCTION_ELEMENT_WIDTH
    );

    packed_auction_bytes
        .chunks(AUCTION_ELEMENT_WIDTH)
        .map(|chunk| {
            let mut chunk_array = [0; AUCTION_ELEMENT_WIDTH];
            chunk_array.copy_from_slice(chunk);
            let mut result = StableXAuctionElement::from_bytes(&chunk_array);
            let order_counter = user_order_counts
                .entry(result.order.account_id)
                .or_insert(0);
            result.order.batch_information = Some(BatchInformation {
                slot_index: *order_counter,
                slot: U256::from(0),
            });
            *order_counter += 1;
            result
        })
        .filter(|x| x.in_auction(index) && x.order.sell_amount > 0)
        .collect()
}

fn auction_elements_to_account_state<'a, Iter>(auction_elements: Iter) -> AccountState
where
    Iter: Iterator<Item = &'a StableXAuctionElement>,
{
    let mut account_state = AccountState::default();
    for element in auction_elements.into_iter() {
        account_state.modify_balance(element.order.account_id, element.order.sell_token, |x| {
            *x = element.sell_token_balance
        });
    }
    account_state
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
            let order_batch_info = orders[order_index]
                .batch_information
                .as_ref()
                .expect("StableX Orders must have Batch Information");
            order_ids.push(order_batch_info.slot_index as u64);
            volumes.push(U128::from(buy_amount.to_be_bytes()));
        }
    }
    (owners, order_ids, volumes)
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use dfusion_core::models::util::map_from_slice;
    use dfusion_core::models::BatchInformation;

    #[test]
    #[should_panic]
    fn encode_execution_fails_on_order_without_batch_info() {
        let insufficient_order = Order {
            batch_information: None,
            account_id: H160::from_low_u64_be(1),
            sell_token: 0,
            buy_token: 1,
            sell_amount: 1,
            buy_amount: 1,
        };
        encode_execution_for_contract(vec![insufficient_order], vec![1]);
    }

    #[test]
    #[should_panic]
    fn encode_execution_fails_on_inconsistent_results() {
        let some_reasonable_order = Order {
            batch_information: Some(BatchInformation {
                slot_index: 0,
                slot: U256::from(0),
            }),
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
            batch_information: Some(BatchInformation {
                slot_index: 0,
                slot: U256::from(0),
            }),
            account_id: address_1,
            sell_token: 0,
            buy_token: 2,
            sell_amount: 1,
            buy_amount: 2,
        };
        let order_2 = Order {
            batch_information: Some(BatchInformation {
                slot_index: 1,
                slot: U256::from(0),
            }),
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
    fn generic_parse_auction_data_test() {
        let bytes: Vec<u8> = vec![
            // order 1
            0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, // user: 20 elements
            0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            0, 0, 3, // sellTokenBalance: 3, 32 elements
            1, 2, // buyToken: 256+2,
            1, 1, // sellToken: 256+1, 56
            0, 0, 0, 2, // validFrom: 2
            0, 0, 1, 5, // validUntil: 256+5 64
            0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 2, // priceNumerator: 258
            0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 3, // priceDenominator: 259
            0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 1, // remainingAmount: 2**8 + 1 = 257
            // order 2
            0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, // user:
            0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            0, 0, 3, // sellTokenBalance: 3
            1, 2, // buyToken: 256+2
            1, 1, // sellToken: 256+1
            0, 0, 0, 2, // validFrom: 2
            0, 0, 1, 5, // validUntil: 256+5
            0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 2, // priceNumerator: 258;
            0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 3, // priceDenominator: 259
            0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 0, // remainingAmount: 2**8 = 256
        ];
        let mut account_state = AccountState::default();

        let order_1 = Order {
            batch_information: Some(BatchInformation {
                slot_index: 0,
                slot: U256::from(0),
            }),
            account_id: H160::from_low_u64_be(1),
            sell_token: 257,
            buy_token: 258,
            sell_amount: 257,
            buy_amount: 257,
        };
        let order_2 = Order {
            batch_information: Some(BatchInformation {
                slot_index: 1,
                slot: U256::from(0),
            }),
            account_id: H160::from_low_u64_be(1),
            sell_token: 257,
            buy_token: 258,
            sell_amount: 256,
            buy_amount: 256,
        };
        let relevant_orders: Vec<Order> = vec![order_1, order_2];
        account_state.modify_balance(H160::from_low_u64_be(1), 257, |x| *x = 3);
        assert_eq!(
            (account_state, relevant_orders),
            get_auction_data(&bytes, U256::from(3))
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
