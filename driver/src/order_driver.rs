use crate::contract::SnappContract;
use crate::db_interface::DbInterface;
use crate::error::{DriverError, ErrorKind};
use crate::models::{Order, RollingHashable, Serializable};
use crate::models;
use crate::price_finding::{PriceFinding, Solution};
use crate::util;

use web3::types::{H256, U256};

pub fn run_order_listener<D, C>(
    db: &D, 
    contract: &C, 
    price_finder: &mut Box<PriceFinding>
) -> Result<bool, DriverError>
    where   D: DbInterface,
            C: SnappContract,
{
    let auction_slot = contract.get_current_auction_slot()?;

    println!("Current top auction slot is {:?}", auction_slot);
    let slot = util::find_first_unapplied_slot(
        auction_slot, 
        Box::new(&|i| contract.has_auction_slot_been_applied(i))
    )?;
    if slot <= auction_slot {
        println!("Highest unprocessed auction slot is {:?}", slot);
        if util::can_process(slot, contract,
            Box::new(&|i| contract.creation_block_for_auction_slot(i))
        )? {
            println!("Processing auction slot {:?}", slot);
            let state_root = contract.get_current_state_root()?;
            let contract_order_hash = contract.order_hash_for_slot(slot)?;
            let mut state = db.get_current_balances(&state_root)?;

            let orders = db.get_orders_of_slot(slot.low_u32())?;
            let order_hash = orders.rolling_hash();
            if order_hash != contract_order_hash {
                return Err(DriverError::new(
                    &format!("Pending order hash from contract ({}), didn't match the one found in db ({})", 
                    contract_order_hash, order_hash), ErrorKind::StateError
                ));
            }

            let solution = if orders.len() > 0 {
                price_finder.find_prices(&orders, &state).unwrap_or_else(|e| {
                    println!("Error computing result: {}\n Falling back to trivial solution", e);
                    Solution {
                        surplus: U256::zero(),
                        prices: vec![0; models::TOKENS as usize],
                        executed_sell_amounts: vec![0; orders.len()],
                        executed_buy_amounts: vec![0; orders.len()],
                    }
                })
            } else {
                println!("No orders in batch. Falling back to trivial solution");
                Solution {
                    surplus: U256::zero(),
                    prices: vec![0; models::TOKENS as usize],
                    executed_sell_amounts: vec![0; orders.len()],
                    executed_buy_amounts: vec![0; orders.len()],
                }
            };
            state.balances = compute_updated_balances(&state.balances, &orders, &solution);
            let new_state_root = H256::from(state.rolling_hash());
            
            println!("New State_hash is {}, Solution: {:?}", new_state_root, solution);
            contract.apply_auction(slot, state_root, new_state_root, order_hash, solution.bytes())?;
            return Ok(true);
        } else {
            println!("Need to wait before processing auction slot {:?}", slot);
        }
    }
    Ok(false)
}

fn compute_updated_balances(
    balances: &Vec<u128>, 
    orders: &Vec<Order>, 
    solution: &Solution
) -> Vec<u128> {
    let mut result = balances.clone();
    for (index, order) in orders.iter().enumerate() {
        let buy_volume = solution.executed_buy_amounts[index];
        let buy_index = util::balance_index(order.buy_token, order.account_id);
        result[buy_index] += buy_volume;

        let sell_volume = solution.executed_sell_amounts[index];
        let sell_index = util::balance_index(order.sell_token, order.account_id);
        result[sell_index] -= sell_volume;
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::contract::tests::SnappContractMock;
    use crate::models::tests::create_order_for_test;
    use crate::db_interface::tests::DbInterfaceMock;
    use crate::price_finding::price_finder_interface::tests::PriceFindingMock;
    use mock_it::Matcher::*;
    use web3::types::U256;

    #[test]
    fn computes_updated_balance_on_example_with_equal_buy_and_sell(){
        let balances = vec![100; 70];
        let solution = Solution {
            surplus: U256::from_dec_str("0").unwrap(),
            prices: vec![1, 2],
            executed_sell_amounts: vec![1, 1],
            executed_buy_amounts: vec![1, 1],
        };
        let order_1 = Order{
          slot_index: 1,
          account_id: 1,
          sell_token: 0,
          buy_token: 1,
          sell_amount: 4,
          buy_amount: 5,
        };
        let order_2 = Order{
          slot_index: 1,
          account_id: 0,
          sell_token: 1,
          buy_token: 0,
          sell_amount: 5,
          buy_amount: 4,
        };
        let orders = vec![order_1, order_2];

        let mut updated_balances = balances.clone();
        updated_balances[0] = 101;
        updated_balances[1] = 99;
        updated_balances[30] = 99;
        updated_balances[31] = 101;
        assert_eq!(compute_updated_balances(&balances, &orders, &solution), updated_balances);
    }
    #[test]
    fn applies_current_state_if_unapplied_and_enough_blocks_passed() {
        let slot = U256::from(1);
        let state_hash = H256::zero();
        let orders = vec![create_order_for_test(1), create_order_for_test(2)];
        let state = models::State {
            state_hash: format!("{:x}", state_hash),
            state_index: 1,
            balances: vec![100; ((models::TOKENS as u16) * 2) as usize],
        };

        let contract = SnappContractMock::new();
        contract.get_current_auction_slot.given(()).will_return(Ok(slot));
        contract.has_auction_slot_been_applied.given(slot).will_return(Ok(false));
        contract.has_auction_slot_been_applied.given(slot - 1).will_return(Ok(true));
        contract.creation_block_for_auction_slot.given(slot).will_return(Ok(U256::from(10)));
        contract.get_current_block_number.given(()).will_return(Ok(U256::from(34)));
        contract.order_hash_for_slot.given(slot).will_return(Ok(orders.rolling_hash()));
        contract.get_current_state_root.given(()).will_return(Ok(state_hash));
        contract.apply_auction.given((slot, Any, Any, Any, Any)).will_return(Ok(()));

        let db = DbInterfaceMock::new();
        db.get_orders_of_slot.given(1).will_return(Ok(orders.clone()));
        db.get_current_balances.given(state_hash).will_return(Ok(state.clone()));

        let pf = PriceFindingMock::new();
        let expected_solution = Solution {
            surplus: U256::from_dec_str("0").unwrap(),
            prices: vec![1, 2],
            executed_sell_amounts: vec![0, 2],
            executed_buy_amounts: vec![0, 2],
        };
        pf.find_prices.given((orders, state)).will_return(Ok(expected_solution));
        let mut pf_box : Box<PriceFinding> = Box::new(pf);

        assert_eq!(run_order_listener(&db, &contract, &mut pf_box), Ok(true));
    }

    #[test]
    fn does_not_apply_if_highest_slot_already_applied() {
        let slot = U256::from(1);
        let contract = SnappContractMock::new();
        contract.get_current_auction_slot.given(()).will_return(Ok(slot));
        contract.has_auction_slot_been_applied.given(slot).will_return(Ok(true));

        let db = DbInterfaceMock::new();
        let mut pf : Box<PriceFinding> = Box::new(PriceFindingMock::new());
        assert_eq!(run_order_listener(&db, &contract, &mut pf), Ok(false));
    }

    #[test]
    fn does_not_apply_if_highest_slot_too_close_to_current_block() {
        let slot = U256::from(1);
        let contract = SnappContractMock::new();
        contract.get_current_auction_slot.given(()).will_return(Ok(slot));
        contract.has_auction_slot_been_applied.given(slot).will_return(Ok(false));
        contract.has_auction_slot_been_applied.given(slot-1).will_return(Ok(true));

        contract.creation_block_for_auction_slot.given(slot).will_return(Ok(U256::from(10)));
        contract.get_current_block_number.given(()).will_return(Ok(U256::from(11)));

        let db = DbInterfaceMock::new();
        let mut pf : Box<PriceFinding> = Box::new(PriceFindingMock::new());
        assert_eq!(run_order_listener(&db, &contract, &mut pf), Ok(false));
    }

    #[test]
    fn applies_all_unapplied_states_before_current() {
        let slot = U256::from(1);
        let state_hash = H256::zero();
        let first_orders = vec![create_order_for_test(1), create_order_for_test(2)];
        let second_orders = vec![create_order_for_test(1), create_order_for_test(2)];

        let contract = SnappContractMock::new();
        contract.get_current_auction_slot.given(()).will_return(Ok(slot));

        contract.has_auction_slot_been_applied.given(slot).will_return(Ok(false));
        contract.has_auction_slot_been_applied.given(slot - 1).will_return(Ok(false));

        contract.creation_block_for_auction_slot.given(slot-1).will_return(Ok(U256::from(10)));

        contract.get_current_block_number.given(()).will_return(Ok(U256::from(34)));
        contract.order_hash_for_slot.given(slot-1).will_return(Ok(second_orders.rolling_hash()));

        contract.get_current_state_root.given(()).will_return(Ok(state_hash));
        contract.apply_auction.given((slot - 1, Any, Any, Any, Any)).will_return(Ok(()));

        let state = models::State {
            state_hash: format!("{:x}", state_hash),
            state_index: 1,
            balances: vec![100; ((models::TOKENS as u16) * 2) as usize],
        };

        let db = DbInterfaceMock::new();
        db.get_orders_of_slot.given(0).will_return(Ok(first_orders.clone()));
        db.get_current_balances.given(state_hash).will_return(Ok(state.clone()));
        
        let pf = PriceFindingMock::new();
        let expected_solution = Solution {
            surplus: U256::from_dec_str("0").unwrap(),
            prices: vec![1, 2],
            executed_sell_amounts: vec![0, 2],
            executed_buy_amounts: vec![0, 2],
        };
        pf.find_prices.given((first_orders, state)).will_return(Ok(expected_solution));

        let mut pf_box : Box<PriceFinding> = Box::new(pf);

        assert_eq!(run_order_listener(&db, &contract, &mut pf_box), Ok(true));
        assert_eq!(run_order_listener(&db, &contract, &mut pf_box), Ok(true));
    }

    #[test]
    fn returns_error_if_db_order_hash_doesnt_match_contract_hash() {
        let slot = U256::from(1);
        let state_hash = H256::zero();

        let orders = vec![create_order_for_test(1), create_order_for_test(2)];

        let state = models::State {
            state_hash: format!("{:x}", state_hash),
            state_index: 1,
            balances: vec![100; ((models::TOKENS as u16) * 2) as usize],
        };

        let contract = SnappContractMock::new();
        contract.get_current_auction_slot.given(()).will_return(Ok(slot));
        contract.has_auction_slot_been_applied.given(slot).will_return(Ok(false));
        contract.has_auction_slot_been_applied.given(slot - 1).will_return(Ok(true));

        contract.creation_block_for_auction_slot.given(slot).will_return(Ok(U256::from(10)));
        contract.get_current_block_number.given(()).will_return(Ok(U256::from(34)));
        
        contract.order_hash_for_slot.given(slot).will_return(Ok(H256::zero()));
        contract.get_current_state_root.given(()).will_return(Ok(state_hash));

        let db = DbInterfaceMock::new();
        db.get_orders_of_slot.given(1).will_return(Ok(orders.clone()));
        db.get_current_balances.given(state_hash).will_return(Ok(state.clone()));

        let mut pf : Box<PriceFinding> = Box::new(PriceFindingMock::new());

        let error = run_order_listener(&db, &contract, &mut pf).expect_err("Expected Error");
        assert_eq!(error.kind, ErrorKind::StateError);
    }
}