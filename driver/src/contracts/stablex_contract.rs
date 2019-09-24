#[cfg(test)]
extern crate mock_it;

use std::collections::HashMap;
use std::env;
use std::fs;

use web3::contract::Options;
use web3::futures::Future;
use web3::types::{H160, U128, U256};

use dfusion_core::models::{AccountState, Order, Solution};

use crate::error::DriverError;

use super::base_contract::BaseContract;
use super::stablex_auction_element::StableXAuctionElement;

type Result<T> = std::result::Result<T, DriverError>;

const AUCTION_ELEMENT_WIDTH: usize = 113;

pub struct StableXContractImpl {
    base: BaseContract,
}

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

    fn get_auction_data(&self, index: U256) -> Result<(AccountState, Vec<Order>)> {
        let packed_auction_bytes: Vec<u8> = self
            .base
            .contract
            .query(
                "getEncodedAuctionElements",
                (),
                None,
                Options::default(),
                None,
            )
            .wait()
            .map_err(DriverError::from)?;
        Ok(parse_auction_data(packed_auction_bytes, index))
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
        let (prices, token_ids_for_price) = encode_prices_for_contract(solution.prices);
        let (owners, order_ids, volumes) =
            encode_execution_for_contract(orders, solution.executed_sell_amounts);

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

fn parse_auction_data(packed_auction_bytes: Vec<u8>, index: U256) -> (AccountState, Vec<Order>) {
    // extract packed auction info
    assert_eq!(
        packed_auction_bytes.len() % AUCTION_ELEMENT_WIDTH,
        0,
        "Each auction should be packed in {} bytes",
        AUCTION_ELEMENT_WIDTH
    );

    let mut account_state = AccountState::default();
    let mut order_count = HashMap::new();
    let relevant_orders = packed_auction_bytes
        .chunks(AUCTION_ELEMENT_WIDTH)
        .map(|chunk| {
            let mut chunk_array = [0; AUCTION_ELEMENT_WIDTH];
            chunk_array.copy_from_slice(chunk);
            StableXAuctionElement::from_bytes(&mut order_count, &chunk_array)
        })
    .filter(|x| x.in_auction(index) && x.order.sell_amount > 0) 
        .map(|element| {
            account_state.modify_balance(element.order.account_id, element.order.sell_token, |x| {
                *x = element.sell_token_balance
            });
            element.order
        })
        .collect();
    (account_state, relevant_orders)
}

fn encode_prices_for_contract(price_vector: Vec<u128>) -> (Vec<U128>, Vec<U128>) {
    // Representing the solution's price vector more compactly as:
    // sorted_touched_token_ids, non_zero_prices which are logically bound by index.
    // Example solution.prices = [3, 0, 1] will be transformed into [0, 2], [3, 1]
    let mut ordered_token_ids: Vec<U128> = vec![];
    let mut prices: Vec<U128> = vec![];
    for (token_id, price) in price_vector.into_iter().enumerate() {
        if price > 0 {
            ordered_token_ids.push(U128::from(token_id));
            prices.push(U128::from(price.to_be_bytes()));
        }
    }
    (prices, ordered_token_ids)
}

fn encode_execution_for_contract(
    orders: Vec<Order>,
    executed_sell_amounts: Vec<u128>,
) -> (Vec<H160>, Vec<U128>, Vec<U128>) {
    assert_eq!(
        orders.len(),
        executed_sell_amounts.len(),
        "Received inconsistent auction result data."
    );
    // Note that buy_amounts do not play a role here since all orders are sell orders.
    let mut owners: Vec<H160> = vec![];
    let mut order_ids: Vec<U128> = vec![];
    let mut volumes: Vec<U128> = vec![];
    for (order_index, sell_amount) in executed_sell_amounts.into_iter().enumerate() {
        if sell_amount > 0 {
            // order was touched!
            // Note that above condition is only holds for sell orders.
            owners.push(orders[order_index].account_id);
            let order_batch_info = orders[order_index]
                .batch_information
                .as_ref()
                .expect("StableX Orders must have Batch Information");
            // TODO - using slot_index (u16) for order_id (U128) is temporary and not sustainable.
            order_ids.push(U128::from(order_batch_info.slot_index));
            // all orders are sell orders, so volumes are sell_amounts.
            // TODO - push buy_amount if not sellOrder
            volumes.push(U128::from(sell_amount.to_be_bytes()));
        }
    }
    (owners, order_ids, volumes)
}

#[cfg(test)]
pub mod tests {
    use mock_it::Matcher;
    use mock_it::Matcher::*;
    use mock_it::Mock;

    use dfusion_core::models::BatchInformation;

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
    #[should_panic]
    fn encode_execution_fails_on_order_without_batch_info() {
        let insufficient_order = Order {
            batch_information: None,
            account_id: H160::from(1),
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
            account_id: H160::from(1),
            sell_token: 0,
            buy_token: 1,
            sell_amount: 1,
            buy_amount: 1,
        };
        encode_execution_for_contract(vec![some_reasonable_order], vec![]);
    }

    #[test]
    fn generic_encode_execution_test() {
        let executed_sell_amounts = vec![1, 0];

        let address_1 = H160::from(1);
        let address_2 = H160::from(2);

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

        let zero = U128::from(0);
        let one = U128::from(1);

        let expected_owners = vec![address_1];
        let expected_order_ids = vec![zero];
        let expected_volumes = vec![one];

        let expected_results = (expected_owners, expected_order_ids, expected_volumes);

        assert_eq!(
            encode_execution_for_contract(vec![order_1, order_2], executed_sell_amounts),
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
            1, // is_sell_order: true
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
            1, // is_sell_order: true
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
            account_id: H160::from(1),
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
            account_id: H160::from(1),
            sell_token: 257,
            buy_token: 258,
            sell_amount: 256,
            buy_amount: 256,
        };
        let relevant_orders: Vec<Order> = vec![order_1, order_2];
        account_state.modify_balance(H160::from(1), 257, |x| *x = 3);
        assert_eq!(
            (account_state, relevant_orders),
            parse_auction_data(bytes, U256::from(3))
        );
    }

    #[test]
    fn generic_price_encoding() {
        let price_vector = vec![u128::max_value(), 0, 1];

        let zero = U128::from(0);
        let one = U128::from(1);
        let two = U128::from(2);
        let max = U128::max_value();

        let expected_prices = vec![max, one];
        let expected_token_ids = vec![zero, two];

        assert_eq!(
            encode_prices_for_contract(price_vector),
            (expected_prices, expected_token_ids)
        );
    }
}
