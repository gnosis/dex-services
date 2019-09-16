#[cfg(test)]
extern crate mock_it;

use dfusion_core::models::util::PopFromLogData;
use dfusion_core::models::{AccountState, Order, Solution};

use web3::contract::Options;
use web3::futures::Future;
use web3::types::{H160, U128, U256};

use crate::error::DriverError;

use super::base_contract::BaseContract;

use std::env;
use std::fs;

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

struct AuctionElement {
    valid_from: U256,
    valid_until: U256,
    sell_token_balance: u128,
    order: Order,
}

impl AuctionElement {
    fn in_auction(&self, index: U256) -> bool {
        self.valid_from < index && index <= self.valid_until
    }

    fn from_bytes(bytes: &[u8; 113]) -> Self {
        let mut data_vector = bytes.to_vec();
        let account_id = H160::pop_from_log_data(&mut data_vector);
        let sell_token_balance = u128::pop_from_log_data(&mut data_vector);
        let buy_token = u16::pop_from_log_data(&mut data_vector);
        let sell_token = u16::pop_from_log_data(&mut data_vector);
        let valid_from = U256::from(u32::pop_from_log_data(&mut data_vector));
        let valid_until = U256::from(u32::pop_from_log_data(&mut data_vector));
        let is_sell_order = bool::pop_from_log_data(&mut data_vector);
        let price_numerator = u128::pop_from_log_data(&mut data_vector);
        let price_denominator = u128::pop_from_log_data(&mut data_vector);
        let remaining_amount = u128::pop_from_log_data(&mut data_vector);

        // Todo - Will likely have to compute this differently.
        // TODO - put in own function fn [u128; 3] -> [u128; 2]
        let buy_amount: u128;
        let sell_amount: u128;
        if is_sell_order {
            sell_amount = remaining_amount;
            buy_amount = (price_numerator * remaining_amount) / price_denominator;
        } else {
            buy_amount = remaining_amount;
            sell_amount = (price_denominator * remaining_amount) / price_numerator;
        }

        AuctionElement {
            valid_from,
            valid_until,
            sell_token_balance,
            order: Order {
                batch_information: None, // TODO - can't recover order_id from this information include it in the return of
                account_id,
                buy_token,
                sell_token,
                buy_amount,
                sell_amount,
            },
        }
    }
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

        // extract packed auction info
        assert_eq!(
            packed_auction_bytes.len() % AUCTION_ELEMENT_WIDTH,
            0,
            "Each auction should be packed in 113 bytes"
        );
        let auction_elements: Vec<AuctionElement> = packed_auction_bytes
            .chunks(AUCTION_ELEMENT_WIDTH)
            .map(|chunk| {
                let mut chunk_array = [0; AUCTION_ELEMENT_WIDTH];
                chunk_array.copy_from_slice(chunk);
                AuctionElement::from_bytes(&chunk_array)
            })
            .collect();

        let mut account_state = AccountState::default(index);
        let relevant_orders = auction_elements
            .into_iter() // using iter here gives "cannot move out of borrowed content"
            .filter(|x| x.in_auction(index))
            .map(|element| {
                account_state.modify_balance(
                    element.order.account_id,
                    element.order.sell_token,
                    |x| *x += element.sell_token_balance,
                );
                element.order
            })
            .collect();

        Ok((account_state, relevant_orders))
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

    use crate::error::ErrorKind;

    use super::*;
    use dfusion_core::models::BatchInformation;

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
