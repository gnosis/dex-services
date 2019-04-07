use crate::contract::SnappContract;
use crate::db_interface::DbInterface;
use crate::error::{DriverError, ErrorKind};
use crate::models::{RollingHashable, Serializable};
use crate::models;
use crate::price_finding::{PriceFinding, Solution};
use crate::util;

use web3::types::{H256, U256};

pub fn run_order_listener<D, C, PF>(
    db: &D, 
    contract: &C, 
    price_finder: &mut PF
) -> Result<bool, DriverError>
    where   D: DbInterface,
            C: SnappContract,
            PF: PriceFinding
{
    let auction_slot = contract.get_current_auction_slot()?;

    println!("Current top auction slot is {:?}", auction_slot);
    let slot = util::find_first_unapplied_slot(
        auction_slot + 1, 
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
            contract.apply_auction(slot, state_root, new_state_root, order_hash, solution.bytes(), solution.bytes())?;
            return Ok(true);
        } else {
            println!("Need to wait before processing auction slot {:?}", slot);
        }
    }
    Ok(false)
}

fn compute_updated_balances(
    balances: &Vec<u128>, 
    orders: &Vec<models::Order>, 
    solution: &Solution
) -> Vec<u128> {
    let mut result = balances.clone();
    for (index, order) in orders.iter().enumerate() {
        let buy_volume = solution.executed_buy_amounts[index];
        let buy_index = (order.account_id as usize * models::TOKENS as usize) + order.buy_token as usize;
        result[buy_index] += buy_volume;

        let sell_volume = solution.executed_sell_amounts[index];
        let sell_index = (order.account_id as usize * models::TOKENS as usize) + order.buy_token as usize;
        result[sell_index] -= sell_volume;
    }
    result
}