use crate::contract::SnappContract;
use crate::error::DriverError;
use crate::price_finding::{PriceFinding, Solution};
use crate::util::{find_first_unapplied_slot, can_process, hash_consistency_check};

use dfusion_core::database::DbInterface;
use dfusion_core::models::{
    RollingHashable, Serializable, ConcatenatingHashable, AccountState, Order, StandingOrder,
    TOKENS,
};

use web3::types::{U256, U128};

pub fn run_order_listener<D, C>(
    db: &D, 
    contract: &C,
    price_finder: &mut PriceFinding
) -> Result<bool, DriverError>
    where   D: DbInterface,
            C: SnappContract,
{
    let auction_slot = contract.get_current_auction_slot()?;

    info!("Current top auction slot is {:?}", auction_slot);
    let slot = find_first_unapplied_slot(
        auction_slot,
        &|i| contract.has_auction_slot_been_applied(i)
    )?;
    if slot <= auction_slot {
        info!("Highest unprocessed auction slot is {:?}", slot);
        if can_process(slot, contract,
            &|i| contract.creation_timestamp_for_auction_slot(i)
        )? {
            info!("Processing auction slot {:?}", slot);
            let state_root = contract.get_current_state_root()?;
            let non_reserved_orders_hash_from_contract = contract.order_hash_for_slot(slot)?;
            let mut state = db.get_current_balances(&state_root)?;

            let mut orders = db.get_orders_of_slot(slot.low_u32())?;
            let non_reserved_orders_hash = orders.rolling_hash(0);
            hash_consistency_check(non_reserved_orders_hash, non_reserved_orders_hash_from_contract, "non-reserved-orders")?;

            let standing_orders = db.get_standing_orders_of_slot(slot.low_u32())?;
            
            orders.extend(standing_orders
                .iter()
                .filter(|standing_order| standing_order.num_orders() > 0)
                .flat_map(|standing_order| standing_order.get_orders().clone())
            );
            info!("All Orders: {:?}", orders);

            let standing_order_indexes = batch_index_from_standing_orders(&standing_orders);
            let total_order_hash_from_contract = contract.calculate_order_hash(slot, standing_order_indexes.clone())?;
            let total_order_hash_calculated = standing_orders.concatenating_hash(non_reserved_orders_hash);
            hash_consistency_check(total_order_hash_calculated, total_order_hash_from_contract, "overall-order")?;

            let solution = if !orders.is_empty() {
                price_finder.find_prices(&orders, &state).unwrap_or_else(|e| {
                    error!("Error computing result: {}\n Falling back to trivial solution", e);
                    Solution {
                        surplus: U256::zero(),
                        prices: vec![0; TOKENS as usize],
                        executed_sell_amounts: vec![0; orders.len()],
                        executed_buy_amounts: vec![0; orders.len()],
                    }
                })
            } else {
                warn!("No orders in batch. Falling back to trivial solution");
                Solution {
                    surplus: U256::zero(),
                    prices: vec![0; TOKENS as usize],
                    executed_sell_amounts: vec![0; orders.len()],
                    executed_buy_amounts: vec![0; orders.len()],
                }
            };

            // Compute updated balances
            update_balances(&mut state, &orders, &solution);
            let new_state_root = state.rolling_hash(state.state_index.low_u32() + 1);
            
            info!("New AccountState hash is {}, Solution: {:?}", new_state_root, solution);

            contract.apply_auction(slot, state_root, new_state_root, total_order_hash_from_contract, standing_order_indexes, solution.bytes())?;
            return Ok(true);
        } else {
            info!("Need to wait before processing auction slot {:?}", slot);
        }
    }
    Ok(false)
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
        standing_orders.iter().map(|o| U128::from(o.batch_index)).collect() 
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::contract::tests::SnappContractMock;
    use dfusion_core::database::tests::DbInterfaceMock;
    use dfusion_core::models::NUM_RESERVED_ACCOUNTS;
    use dfusion_core::models::order::tests::create_order_for_test;
    use crate::price_finding::price_finder_interface::tests::PriceFindingMock;
    use mock_it::Matcher::*;
    use web3::types::{H256, U128, U256};
    use crate::error::{ErrorKind};
    use crate::price_finding::error::{PriceFindingError};

    #[test]
    fn applies_current_state_if_unapplied_and_enough_blocks_passed() {
        let slot = U256::from(1);
        let state_hash = H256::zero();
        let orders = vec![create_order_for_test(), create_order_for_test()];
        let state = AccountState::new(
            state_hash,
            U256::one(),
            vec![100; (TOKENS * 2) as usize],
            TOKENS,
        );
        let contract = SnappContractMock::new();
        contract.get_current_auction_slot.given(()).will_return(Ok(slot));
        contract.has_auction_slot_been_applied.given(slot).will_return(Ok(false));
        contract.has_auction_slot_been_applied.given(slot - 1).will_return(Ok(true));
        contract.creation_timestamp_for_auction_slot.given(slot).will_return(Ok(U256::from(10)));
        contract.get_current_block_timestamp.given(()).will_return(Ok(U256::from(200)));
        contract.order_hash_for_slot.given(slot).will_return(Ok(orders.rolling_hash(0)));
        contract.get_current_state_root.given(()).will_return(Ok(state_hash));
        contract.calculate_order_hash.given((slot, Any)).will_return(Ok(H256::from("0x438d54b20a21fa0b2f8f176c86446d9db7067f6e68a1e58c22873544eb20d72c")));

        contract.apply_auction.given((slot, Any, Any, Any, Any, Any)).will_return(Ok(()));
        let standing_orders = StandingOrder::empty_array();
        let db = DbInterfaceMock::new();
        db.get_orders_of_slot.given(1).will_return(Ok(orders.clone()));
        db.get_standing_orders_of_slot.given(1).will_return(Ok(standing_orders));
        db.get_current_balances.given(state_hash).will_return(Ok(state.clone()));

        let mut pf = PriceFindingMock::new();
        let expected_solution = Solution {
            surplus: U256::from_dec_str("0").unwrap(),
            prices: vec![1, 2],
            executed_sell_amounts: vec![0, 2],
            executed_buy_amounts: vec![0, 2],
        };
        pf.find_prices.given((orders, state)).will_return(Ok(expected_solution));

        assert_eq!(run_order_listener(&db, &contract, &mut pf), Ok(true));
    }

    #[test]
    fn does_not_apply_if_highest_slot_already_applied() {
        let slot = U256::from(1);
        let contract = SnappContractMock::new();
        contract.get_current_auction_slot.given(()).will_return(Ok(slot));
        contract.has_auction_slot_been_applied.given(slot).will_return(Ok(true));

        let db = DbInterfaceMock::new();
        let mut pf = PriceFindingMock::new();

        assert_eq!(run_order_listener(&db, &contract, &mut pf), Ok(false));
    }

    #[test]
    fn does_not_apply_if_highest_slot_too_close_to_current_block() {
        let slot = U256::from(1);
        let contract = SnappContractMock::new();
        contract.get_current_auction_slot.given(()).will_return(Ok(slot));
        contract.has_auction_slot_been_applied.given(slot).will_return(Ok(false));
        contract.has_auction_slot_been_applied.given(slot-1).will_return(Ok(true));

        contract.creation_timestamp_for_auction_slot.given(slot).will_return(Ok(U256::from(10)));
        contract.get_current_block_timestamp.given(()).will_return(Ok(U256::from(11)));

        let db = DbInterfaceMock::new();
        let mut pf = PriceFindingMock::new();

        assert_eq!(run_order_listener(&db, &contract, &mut pf), Ok(false));
    }

    #[test]
    fn applies_all_unapplied_states_before_current() {
        let slot = U256::from(1);
        let state_hash = H256::zero();
        let first_orders = vec![create_order_for_test(), create_order_for_test()];
        let second_orders = vec![create_order_for_test(), create_order_for_test()];

        let contract = SnappContractMock::new();
        contract.get_current_auction_slot.given(()).will_return(Ok(slot));

        contract.has_auction_slot_been_applied.given(slot).will_return(Ok(false));
        contract.has_auction_slot_been_applied.given(slot - 1).will_return(Ok(false));

        contract.creation_timestamp_for_auction_slot.given(slot-1).will_return(Ok(U256::from(10)));

        contract.get_current_block_timestamp.given(()).will_return(Ok(U256::from(200)));
        contract.order_hash_for_slot.given(slot-1).will_return(Ok(second_orders.rolling_hash(0)));
        contract.calculate_order_hash.given((slot-1, Any)).will_return(Ok(H256::from("0x438d54b20a21fa0b2f8f176c86446d9db7067f6e68a1e58c22873544eb20d72c")));

        contract.get_current_state_root.given(()).will_return(Ok(state_hash));
        contract.apply_auction.given((slot - 1, Any, Any, Any, Any, Any)).will_return(Ok(()));

        let state = AccountState::new(
            state_hash,
            U256::one(),
            vec![100; (TOKENS * 2) as usize],
            TOKENS,
        );
        let standing_orders = StandingOrder::empty_array();
        let db = DbInterfaceMock::new();
        db.get_orders_of_slot.given(0).will_return(Ok(first_orders.clone()));
        db.get_standing_orders_of_slot.given(0).will_return(Ok(standing_orders));
        db.get_current_balances.given(state_hash).will_return(Ok(state.clone()));

        let mut pf = PriceFindingMock::new();
        let expected_solution = Solution {
            surplus: U256::from_dec_str("0").unwrap(),
            prices: vec![1, 2],
            executed_sell_amounts: vec![0, 2],
            executed_buy_amounts: vec![0, 2],
        };
        pf.find_prices.given((first_orders, state)).will_return(Ok(expected_solution));

        assert_eq!(run_order_listener(&db, &contract, &mut pf), Ok(true));
        assert_eq!(run_order_listener(&db, &contract, &mut pf), Ok(true));
    }

    #[test]
    fn returns_error_if_db_order_hash_doesnt_match_contract_hash() {
        let slot = U256::from(1);
        let state_hash = H256::zero();

        let orders = vec![create_order_for_test(), create_order_for_test()];

        let state = AccountState::new(
            state_hash,
            U256::one(),
            vec![100; (TOKENS * 2) as usize],
            TOKENS,
        );

        let contract = SnappContractMock::new();
        contract.get_current_auction_slot.given(()).will_return(Ok(slot));
        contract.has_auction_slot_been_applied.given(slot).will_return(Ok(false));
        contract.has_auction_slot_been_applied.given(slot - 1).will_return(Ok(true));

        contract.creation_timestamp_for_auction_slot.given(slot).will_return(Ok(U256::from(10)));
        contract.get_current_block_timestamp.given(()).will_return(Ok(U256::from(200)));
        
        contract.order_hash_for_slot.given(slot).will_return(Ok(H256::zero()));
        contract.get_current_state_root.given(()).will_return(Ok(state_hash));

        let db = DbInterfaceMock::new();
        db.get_orders_of_slot.given(1).will_return(Ok(orders.clone()));
        db.get_current_balances.given(state_hash).will_return(Ok(state.clone()));

        let mut pf = PriceFindingMock::new();

        let error = run_order_listener(&db, &contract, &mut pf).expect_err("Expected Error");
        assert_eq!(error.kind, ErrorKind::StateError);
    }

    #[test]
    fn considers_standing_orders() {
        let slot = U256::from(1);
        let state_hash = H256::zero();
        let standing_order = StandingOrder::new(
            1, U256::zero(), vec![create_order_for_test(), create_order_for_test()]
        );

        let state = AccountState::new(
            state_hash,
            U256::one(),
            vec![100; (TOKENS * 2) as usize],
            TOKENS,
        );

        let contract = SnappContractMock::new();
        contract.get_current_auction_slot.given(()).will_return(Ok(slot));
        contract.has_auction_slot_been_applied.given(slot).will_return(Ok(false));
        contract.has_auction_slot_been_applied.given(slot - 1).will_return(Ok(true));
        contract.creation_timestamp_for_auction_slot.given(slot).will_return(Ok(U256::from(10)));
        contract.get_current_block_timestamp.given(()).will_return(Ok(U256::from(200)));
        contract.order_hash_for_slot.given(slot).will_return(Ok(H256::zero()));
        contract.calculate_order_hash.given((slot, Any)).will_return(Ok(H256::from("0x6bdda4f03645914c836a16ba8565f26dffb7bec640b31e1f23e0b3b22f0a64ae")));
        contract.get_current_state_root.given(()).will_return(Ok(state_hash));
        contract.apply_auction.given((slot, Any, Any, Any, Any, Any)).will_return(Ok(()));

        let mut standing_orders = StandingOrder::empty_array();
        standing_orders[1] = standing_order.clone();
        let db = DbInterfaceMock::new();
        db.get_orders_of_slot.given(1).will_return(Ok(vec![]));
        db.get_standing_orders_of_slot.given(1).will_return(Ok(standing_orders.clone()));
        db.get_current_balances.given(state_hash).will_return(Ok(state.clone()));

        let mut pf = PriceFindingMock::new();
        pf.find_prices
            .given((standing_order.get_orders().clone(), state))
            .will_return(Err(PriceFindingError::from("Trivial solution is fine")));

        assert_eq!(run_order_listener(&db, &contract, &mut pf), Ok(true));
    }

    #[test]
    fn test_get_standing_orders_indexes(){
        let standing_order = StandingOrder::new(
            1, U256::from(3), vec![create_order_for_test(), create_order_for_test()]
        );
        let empty_order = StandingOrder::new(
            0, U256::zero(), vec![]
        );
        let mut standing_orders = vec![empty_order; NUM_RESERVED_ACCOUNTS as usize];
        standing_orders[1] = standing_order.clone();
        let mut standing_order_indexes = vec![U128::zero(); NUM_RESERVED_ACCOUNTS as usize];
        standing_order_indexes[1] = U128::from(3);
        assert_eq!(batch_index_from_standing_orders(&standing_orders), standing_order_indexes);
    }

    #[test]
    fn test_update_balances(){
        let mut state = AccountState::new(
            H256::zero(),
            U256::one(),
            vec![100; 70],
            TOKENS,
        );
        let solution = Solution {
            surplus: U256::from_dec_str("0").unwrap(),
            prices: vec![1, 2],
            executed_sell_amounts: vec![1, 1],
            executed_buy_amounts: vec![1, 1],
        };
        let order_1 = Order{
          batch_information: None,
          account_id: 1,
          sell_token: 0,
          buy_token: 1,
          sell_amount: 4,
          buy_amount: 5,
        };
        let order_2 = Order{
          batch_information: None,
          account_id: 0,
          sell_token: 1,
          buy_token: 0,
          sell_amount: 5,
          buy_amount: 4,
        };
        let orders = vec![order_1, order_2];

        update_balances(&mut state, &orders, &solution);
        assert_eq!(state.read_balance(0, 0), 101);
        assert_eq!(state.read_balance(1, 0), 99);
        assert_eq!(state.read_balance(0, 1), 99);
        assert_eq!(state.read_balance(1, 1), 101);
    }
}