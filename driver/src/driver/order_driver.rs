use crate::contracts::snapp_contract::SnappContract;
use crate::error::{DriverError, ErrorKind};
use crate::price_finding::PriceFinding;
use crate::snapp::SnappSolution;
use crate::util::{
    batch_processing_state, find_first_unapplied_slot, hash_consistency_check, ProcessingState,
};

use dfusion_core::database::DbInterface;
use dfusion_core::models::{
    AccountState, ConcatenatingHashable, Order, RollingHashable, Serializable, Solution,
    StandingOrder,
};

use log::{error, info, warn};

use web3::types::{H256, U128, U256};

use std::collections::HashMap;

struct AuctionBid {
    previous_state: H256,
    new_state: H256,
    solution: Solution,
}

pub struct OrderProcessor<'a> {
    auction_bids: HashMap<U256, AuctionBid>,
    db: &'a dyn DbInterface,
    contract: &'a dyn SnappContract,
    price_finder: &'a mut dyn PriceFinding,
}

#[derive(Eq, PartialEq, Debug)]
pub enum ProcessResult {
    NoAction,
    AuctionBid(U256),
    AuctionApplied(U256),
}

impl<'a> OrderProcessor<'a> {
    pub fn new(
        db: &'a dyn DbInterface,
        contract: &'a dyn SnappContract,
        price_finder: &'a mut dyn PriceFinding,
    ) -> Self {
        OrderProcessor {
            auction_bids: HashMap::new(),
            db,
            contract,
            price_finder,
        }
    }

    pub fn run(&mut self) -> Result<ProcessResult, DriverError> {
        let auction_slot = self.contract.get_current_auction_slot()?;

        info!("Current top auction slot is {:?}", auction_slot);
        let slot = find_first_unapplied_slot(auction_slot, &|i| {
            self.contract.has_auction_slot_been_applied(i)
        })?;
        if slot <= auction_slot {
            info!("Highest unprocessed auction slot is {:?}", slot);
            let processing_state = batch_processing_state(slot, self.contract, &|i| {
                self.contract.creation_timestamp_for_auction_slot(i)
            })?;
            match processing_state {
                ProcessingState::TooEarly => {
                    info!("Need to wait before processing auction slot {:?}", slot)
                }
                ProcessingState::AcceptsBids => {
                    if !self.auction_bids.contains_key(&slot) {
                        self.bid_for_auction(slot)?;
                        return Ok(ProcessResult::AuctionBid(slot));
                    }
                    info!("Already bid for auction slot {:?}", slot);
                }
                ProcessingState::AcceptsSolution => {
                    if let Some(bid) = self.auction_bids.get(&slot) {
                        self.contract.apply_auction(
                            slot,
                            bid.previous_state,
                            bid.new_state,
                            bid.solution.bytes(),
                        )?;
                        return Ok(ProcessResult::AuctionApplied(slot));
                    }
                    return Err(DriverError::new(
                        &format!("Cannot find saved bid for auction slot {:?}", slot),
                        ErrorKind::StateError,
                    ));
                }
            }
        }
        Ok(ProcessResult::NoAction)
    }

    fn bid_for_auction(&mut self, auction_index: U256) -> Result<(), DriverError> {
        info!("Processing auction slot {:?}", auction_index);
        let state_root = self.contract.get_current_state_root()?;
        let non_reserved_orders_hash_from_contract =
            self.contract.order_hash_for_slot(auction_index)?;
        let mut state = self.db.get_balances_for_state_root(&state_root)?;

        let mut orders = self.db.get_orders_of_slot(&auction_index)?;
        let non_reserved_orders_hash = orders.rolling_hash(0);
        hash_consistency_check(
            non_reserved_orders_hash,
            non_reserved_orders_hash_from_contract,
            "non-reserved-orders",
        )?;

        let standing_orders = self.db.get_standing_orders_of_slot(&auction_index)?;

        orders.extend(
            standing_orders
                .iter()
                .filter(|standing_order| standing_order.num_orders() > 0)
                .flat_map(|standing_order| standing_order.get_orders().clone()),
        );
        info!("All Orders: {:?}", orders);

        let standing_order_indexes = batch_index_from_standing_orders(&standing_orders);
        let total_order_hash_from_contract = self
            .contract
            .calculate_order_hash(auction_index, standing_order_indexes.clone())?;
        let total_order_hash_calculated =
            standing_orders.concatenating_hash(non_reserved_orders_hash);
        hash_consistency_check(
            total_order_hash_calculated,
            total_order_hash_from_contract,
            "overall-order",
        )?;

        let solution = if !orders.is_empty() {
            self.price_finder
                .find_prices(&orders, &state)
                .unwrap_or_else(|e| {
                    error!(
                        "Error computing result: {}\n Falling back to trivial solution",
                        e
                    );
                    Solution::trivial(orders.len())
                })
        } else {
            warn!("No orders in batch. Falling back to trivial solution");
            Solution::trivial(orders.len())
        };

        // Compute updated balances
        update_balances(&mut state, &orders, &solution);
        let new_state_root = state.rolling_hash(state.state_index.low_u32() + 1);

        let objective_value = match solution.snapp_objective_value(&orders) {
            Ok(objective_value) => objective_value,
            Err(err) => {
                warn!(
                    "Error calculating objective value: {}. May indicate an invalid solution",
                    err
                );
                U256::zero()
            }
        };

        info!(
            "New AccountState hash is {}, Solution: {:?}",
            new_state_root, solution
        );

        self.contract.auction_solution_bid(
            auction_index,
            state_root,
            new_state_root,
            total_order_hash_from_contract,
            standing_order_indexes,
            objective_value,
        )?;

        self.auction_bids.insert(
            auction_index,
            AuctionBid {
                previous_state: state_root,
                new_state: new_state_root,
                solution,
            },
        );

        Ok(())
    }
}

fn update_balances(state: &mut AccountState, orders: &[Order], solution: &Solution) {
    for (i, order) in orders.iter().enumerate() {
        let buy_volume = solution.executed_buy_amounts[i];
        state.increment_balance(order.buy_token, order.account_id, buy_volume);

        let sell_volume = solution.executed_sell_amounts[i];
        state.decrement_balance(order.sell_token, order.account_id, sell_volume);
    }
}

fn batch_index_from_standing_orders(standing_orders: &[StandingOrder]) -> Vec<U128> {
    standing_orders
        .iter()
        .map(|o| o.batch_index.as_u128().into())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::contracts::snapp_contract::tests::SnappContractMock;
    use crate::error::ErrorKind;
    use crate::price_finding::error::PriceFindingError;
    use crate::price_finding::price_finder_interface::tests::PriceFindingMock;
    use dfusion_core::database::tests::DbInterfaceMock;
    use dfusion_core::models::order::test_util::create_order_for_test;
    use dfusion_core::models::NUM_RESERVED_ACCOUNTS;
    use mock_it::Matcher::*;
    use std::str::FromStr;
    use web3::types::{H160, H256, U256};

    const NUM_TOKENS: u16 = 10;

    #[test]
    fn bids_for_and_applies_auction_if_unapplied_and_enough_blocks_passed() {
        let slot = U256::from(1);
        let state_hash = H256::zero();
        let orders = vec![create_order_for_test(), create_order_for_test()];
        let state = AccountState::new(
            state_hash,
            U256::one(),
            vec![100; (NUM_TOKENS * 2) as usize],
            NUM_TOKENS,
        );
        let contract = SnappContractMock::default();
        contract
            .get_current_auction_slot
            .given(())
            .will_return(Ok(slot));
        contract
            .has_auction_slot_been_applied
            .given(slot)
            .will_return(Ok(false));
        contract
            .has_auction_slot_been_applied
            .given(slot - 1)
            .will_return(Ok(true));
        contract
            .creation_timestamp_for_auction_slot
            .given(slot)
            .will_return(Ok(U256::from(10)));
        let get_current_block_timestamp = contract.get_current_block_timestamp.given(());
        get_current_block_timestamp.will_return(Ok(U256::from(200)));
        contract
            .order_hash_for_slot
            .given(slot)
            .will_return(Ok(orders.rolling_hash(0)));
        contract
            .get_current_state_root
            .given(())
            .will_return(Ok(state_hash));
        contract
            .calculate_order_hash
            .given((slot, Any))
            .will_return(Ok(H256::from_str(
                "438d54b20a21fa0b2f8f176c86446d9db7067f6e68a1e58c22873544eb20d72c",
            )
            .unwrap()));

        contract
            .auction_solution_bid
            .given((slot, Any, Any, Any, Any, U256::zero()))
            .will_return(Ok(()));
        let standing_orders = StandingOrder::empty_array();
        let db = DbInterfaceMock::new();
        db.get_orders_of_slot
            .given(U256::one())
            .will_return(Ok(orders.clone()));
        db.get_standing_orders_of_slot
            .given(U256::one())
            .will_return(Ok(standing_orders));
        db.get_balances_for_state_root
            .given(state_hash)
            .will_return(Ok(state.clone()));

        let mut pf = PriceFindingMock::default();
        let expected_solution = Solution {
            prices: vec![1, 2],
            executed_sell_amounts: vec![0, 2],
            executed_buy_amounts: vec![0, 2],
        };
        pf.find_prices
            .given((orders, state))
            .will_return(Ok(expected_solution));

        let mut processor = OrderProcessor::new(&db, &contract, &mut pf);
        assert_eq!(processor.run(), Ok(ProcessResult::AuctionBid(slot)));

        get_current_block_timestamp.will_return(Ok(U256::from(400)));
        contract
            .apply_auction
            .given((slot, Any, Any, Any))
            .will_return(Ok(()));

        assert_eq!(processor.run(), Ok(ProcessResult::AuctionApplied(slot)));
    }

    #[test]
    fn does_not_bid_if_highest_slot_already_applied() {
        let slot = U256::from(1);
        let contract = SnappContractMock::default();
        contract
            .get_current_auction_slot
            .given(())
            .will_return(Ok(slot));
        contract
            .has_auction_slot_been_applied
            .given(slot)
            .will_return(Ok(true));

        let db = DbInterfaceMock::new();
        let mut pf = PriceFindingMock::default();

        let mut processor = OrderProcessor::new(&db, &contract, &mut pf);
        assert_eq!(processor.run(), Ok(ProcessResult::NoAction));
    }

    #[test]
    fn does_not_bid_if_highest_slot_too_close_to_current_block() {
        let slot = U256::from(1);
        let contract = SnappContractMock::default();
        contract
            .get_current_auction_slot
            .given(())
            .will_return(Ok(slot));
        contract
            .has_auction_slot_been_applied
            .given(slot)
            .will_return(Ok(false));
        contract
            .has_auction_slot_been_applied
            .given(slot - 1)
            .will_return(Ok(true));

        contract
            .creation_timestamp_for_auction_slot
            .given(slot)
            .will_return(Ok(U256::from(10)));
        contract
            .get_current_block_timestamp
            .given(())
            .will_return(Ok(U256::from(11)));

        let db = DbInterfaceMock::new();
        let mut pf = PriceFindingMock::default();

        let mut processor = OrderProcessor::new(&db, &contract, &mut pf);
        assert_eq!(processor.run(), Ok(ProcessResult::NoAction));
    }

    #[test]
    fn test_does_not_bid_twice_on_same_slot() {
        let slot = U256::from(1);
        let state_hash = H256::zero();
        let orders = vec![create_order_for_test(), create_order_for_test()];
        let state = AccountState::new(
            state_hash,
            U256::one(),
            vec![100; (NUM_TOKENS * 2) as usize],
            NUM_TOKENS,
        );
        let contract = SnappContractMock::default();
        contract
            .get_current_auction_slot
            .given(())
            .will_return(Ok(slot));
        contract
            .has_auction_slot_been_applied
            .given(slot)
            .will_return(Ok(false));
        contract
            .has_auction_slot_been_applied
            .given(slot - 1)
            .will_return(Ok(true));
        contract
            .creation_timestamp_for_auction_slot
            .given(slot)
            .will_return(Ok(U256::from(10)));
        contract
            .get_current_block_timestamp
            .given(())
            .will_return(Ok(U256::from(200)));
        contract
            .order_hash_for_slot
            .given(slot)
            .will_return(Ok(orders.rolling_hash(0)));
        contract
            .get_current_state_root
            .given(())
            .will_return(Ok(state_hash));
        contract
            .calculate_order_hash
            .given((slot, Any))
            .will_return(Ok(H256::from_str(
                "438d54b20a21fa0b2f8f176c86446d9db7067f6e68a1e58c22873544eb20d72c",
            )
            .unwrap()));

        contract
            .auction_solution_bid
            .given((slot, Any, Any, Any, Any, U256::zero()))
            .will_return(Ok(()));
        let standing_orders = StandingOrder::empty_array();
        let db = DbInterfaceMock::new();
        db.get_orders_of_slot
            .given(U256::one())
            .will_return(Ok(orders));
        db.get_standing_orders_of_slot
            .given(U256::one())
            .will_return(Ok(standing_orders));
        db.get_balances_for_state_root
            .given(state_hash)
            .will_return(Ok(state));

        let mut pf = PriceFindingMock::default();
        let mut processor = OrderProcessor::new(&db, &contract, &mut pf);
        assert_eq!(processor.run(), Ok(ProcessResult::AuctionBid(slot)));
        assert_eq!(processor.run(), Ok(ProcessResult::NoAction));
    }

    #[test]
    fn processes_all_unapplied_states_before_current() {
        let slot = U256::from(1);
        let state_hash = H256::zero();
        let first_orders = vec![create_order_for_test(), create_order_for_test()];
        let second_orders = vec![create_order_for_test(), create_order_for_test()];

        let contract = SnappContractMock::default();
        contract
            .get_current_auction_slot
            .given(())
            .will_return(Ok(slot));

        let has_first_slot_been_applied = contract.has_auction_slot_been_applied.given(slot - 1);
        contract
            .has_auction_slot_been_applied
            .given(slot)
            .will_return(Ok(false));

        contract
            .creation_timestamp_for_auction_slot
            .given(slot - 1)
            .will_return(Ok(U256::from(10)));
        contract
            .creation_timestamp_for_auction_slot
            .given(slot)
            .will_return(Ok(U256::from(200)));

        let get_current_block_timestamp = contract.get_current_block_timestamp.given(());
        contract
            .order_hash_for_slot
            .given(slot - 1)
            .will_return(Ok(second_orders.rolling_hash(0)));
        contract
            .calculate_order_hash
            .given((slot - 1, Any))
            .will_return(Ok(H256::from_str(
                "438d54b20a21fa0b2f8f176c86446d9db7067f6e68a1e58c22873544eb20d72c",
            )
            .unwrap()));
        contract
            .order_hash_for_slot
            .given(slot)
            .will_return(Ok(second_orders.rolling_hash(0)));
        contract
            .calculate_order_hash
            .given((slot, Any))
            .will_return(Ok(H256::from_str(
                "438d54b20a21fa0b2f8f176c86446d9db7067f6e68a1e58c22873544eb20d72c",
            )
            .unwrap()));

        contract
            .get_current_state_root
            .given(())
            .will_return(Ok(state_hash));
        contract
            .auction_solution_bid
            .given((slot - 1, Any, Any, Any, Any, U256::zero()))
            .will_return(Ok(()));
        contract
            .auction_solution_bid
            .given((slot, Any, Any, Any, Any, U256::zero()))
            .will_return(Ok(()));
        contract
            .apply_auction
            .given((slot - 1, Any, Any, Any))
            .will_return(Ok(()));

        let state = AccountState::new(
            state_hash,
            U256::one(),
            vec![100; (NUM_TOKENS * 2) as usize],
            NUM_TOKENS,
        );
        let standing_orders = StandingOrder::empty_array();
        let db = DbInterfaceMock::new();
        db.get_orders_of_slot
            .given(slot - 1)
            .will_return(Ok(first_orders.clone()));
        db.get_standing_orders_of_slot
            .given(slot - 1)
            .will_return(Ok(standing_orders.clone()));
        db.get_orders_of_slot
            .given(slot)
            .will_return(Ok(first_orders.clone()));
        db.get_standing_orders_of_slot
            .given(slot)
            .will_return(Ok(standing_orders));

        db.get_balances_for_state_root
            .given(state_hash)
            .will_return(Ok(state.clone()));

        let mut pf = PriceFindingMock::default();
        let expected_solution = Solution {
            prices: vec![1, 2],
            executed_sell_amounts: vec![0, 2],
            executed_buy_amounts: vec![0, 2],
        };
        pf.find_prices
            .given((first_orders, state))
            .will_return(Ok(expected_solution));

        let mut processor = OrderProcessor::new(&db, &contract, &mut pf);
        get_current_block_timestamp.will_return(Ok(U256::from(200)));
        has_first_slot_been_applied.will_return(Ok(false));
        assert_eq!(processor.run(), Ok(ProcessResult::AuctionBid(slot - 1)));

        get_current_block_timestamp.will_return(Ok(U256::from(400)));
        assert_eq!(processor.run(), Ok(ProcessResult::AuctionApplied(slot - 1)));

        // Lastly process newer auction
        has_first_slot_been_applied.will_return(Ok(true));
        assert_eq!(processor.run(), Ok(ProcessResult::AuctionBid(slot)));
    }

    #[test]
    fn returns_error_if_db_order_hash_doesnt_match_contract_hash() {
        let slot = U256::from(1);
        let state_hash = H256::zero();

        let orders = vec![create_order_for_test(), create_order_for_test()];

        let state = AccountState::new(
            state_hash,
            U256::one(),
            vec![100; (NUM_TOKENS * 2) as usize],
            NUM_TOKENS,
        );

        let contract = SnappContractMock::default();
        contract
            .get_current_auction_slot
            .given(())
            .will_return(Ok(slot));
        contract
            .has_auction_slot_been_applied
            .given(slot)
            .will_return(Ok(false));
        contract
            .has_auction_slot_been_applied
            .given(slot - 1)
            .will_return(Ok(true));

        contract
            .creation_timestamp_for_auction_slot
            .given(slot)
            .will_return(Ok(U256::from(10)));
        contract
            .get_current_block_timestamp
            .given(())
            .will_return(Ok(U256::from(200)));

        contract
            .order_hash_for_slot
            .given(slot)
            .will_return(Ok(H256::zero()));
        contract
            .get_current_state_root
            .given(())
            .will_return(Ok(state_hash));
        let db = DbInterfaceMock::new();
        db.get_orders_of_slot
            .given(U256::one())
            .will_return(Ok(orders));
        db.get_balances_for_state_root
            .given(state_hash)
            .will_return(Ok(state));

        let mut pf = PriceFindingMock::default();

        let mut processor = OrderProcessor::new(&db, &contract, &mut pf);
        let error = processor.run().expect_err("Expected Error");
        assert_eq!(error.kind, ErrorKind::StateError);
    }

    #[test]
    fn considers_standing_orders_in_bid() {
        let slot = U256::from(1);
        let state_hash = H256::zero();
        let standing_order = StandingOrder::new(
            H160::from_low_u64_be(1),
            U256::zero(),
            U256::from(3),
            vec![create_order_for_test(), create_order_for_test()],
        );

        let state = AccountState::new(
            state_hash,
            U256::one(),
            vec![100; (NUM_TOKENS * 2) as usize],
            NUM_TOKENS,
        );

        let contract = SnappContractMock::default();
        contract
            .get_current_auction_slot
            .given(())
            .will_return(Ok(slot));
        contract
            .has_auction_slot_been_applied
            .given(slot)
            .will_return(Ok(false));
        contract
            .has_auction_slot_been_applied
            .given(slot - 1)
            .will_return(Ok(true));
        contract
            .creation_timestamp_for_auction_slot
            .given(slot)
            .will_return(Ok(U256::from(10)));
        contract
            .get_current_block_timestamp
            .given(())
            .will_return(Ok(U256::from(200)));
        contract
            .order_hash_for_slot
            .given(slot)
            .will_return(Ok(H256::zero()));
        contract
            .calculate_order_hash
            .given((slot, Any))
            .will_return(Ok(H256::from_str(
                "6bdda4f03645914c836a16ba8565f26dffb7bec640b31e1f23e0b3b22f0a64ae",
            )
            .unwrap()));
        contract
            .get_current_state_root
            .given(())
            .will_return(Ok(state_hash));
        contract
            .auction_solution_bid
            .given((slot, Any, Any, Any, Any, U256::zero()))
            .will_return(Ok(()));

        let mut standing_orders = StandingOrder::empty_array();
        standing_orders[1] = standing_order.clone();
        let db = DbInterfaceMock::new();
        db.get_orders_of_slot
            .given(U256::one())
            .will_return(Ok(vec![]));
        db.get_standing_orders_of_slot
            .given(U256::one())
            .will_return(Ok(standing_orders));
        db.get_balances_for_state_root
            .given(state_hash)
            .will_return(Ok(state.clone()));

        let mut pf = PriceFindingMock::default();
        pf.find_prices
            .given((standing_order.get_orders().clone(), state))
            .will_return(Err(PriceFindingError::from("Trivial solution is fine")));

        let mut processor = OrderProcessor::new(&db, &contract, &mut pf);
        assert_eq!(processor.run(), Ok(ProcessResult::AuctionBid(slot)));
    }

    #[test]
    fn test_get_standing_orders_indexes() {
        let standing_order = StandingOrder::new(
            H160::from_low_u64_be(1),
            U256::from(3),
            U256::from(2),
            vec![create_order_for_test(), create_order_for_test()],
        );
        let empty_order = StandingOrder::new(H160::zero(), U256::zero(), U256::from(2), vec![]);
        let mut standing_orders = vec![empty_order; NUM_RESERVED_ACCOUNTS as usize];
        standing_orders[1] = standing_order;
        let mut standing_order_indexes = vec![U128::zero(); NUM_RESERVED_ACCOUNTS as usize];
        standing_order_indexes[1] = 3.into();
        assert_eq!(
            batch_index_from_standing_orders(&standing_orders),
            standing_order_indexes
        );
    }

    #[test]
    fn test_update_balances() {
        let mut state = AccountState::new(H256::zero(), U256::one(), vec![100; 60], NUM_TOKENS);
        let solution = Solution {
            prices: vec![1, 2],
            executed_sell_amounts: vec![1, 1],
            executed_buy_amounts: vec![1, 1],
        };
        let order_1 = Order {
            batch_information: None,
            account_id: H160::from_low_u64_be(1),
            sell_token: 0,
            buy_token: 1,
            sell_amount: 4,
            buy_amount: 5,
        };
        let order_2 = Order {
            batch_information: None,
            account_id: H160::from_low_u64_be(0),
            sell_token: 1,
            buy_token: 0,
            sell_amount: 5,
            buy_amount: 4,
        };
        let orders = vec![order_1, order_2];

        update_balances(&mut state, &orders, &solution);
        assert_eq!(state.read_balance(0, H160::from_low_u64_be(0)), 101);
        assert_eq!(state.read_balance(1, H160::from_low_u64_be(0)), 99);
        assert_eq!(state.read_balance(0, H160::from_low_u64_be(1)), 99);
        assert_eq!(state.read_balance(1, H160::from_low_u64_be(1)), 101);
    }
}
