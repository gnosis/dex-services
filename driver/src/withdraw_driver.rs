use crate::db_interface::DbInterface;
use crate::contract::SnappContract;

use web3::types::{H256, U256};

use std::error::Error;

fn find_first_unapplied_slot<C>(upper_bound: U256, contract: &C) -> Result<Option<U256>, Box<dyn Error>>
    where C: SnappContract
{
    let mut slot = upper_bound;
    while slot != U256::zero() {
        if contract.has_deposit_slot_been_applied(slot)? {
            return Ok(Some(slot))
        }
        slot = slot - 1;
    }
    Ok(None)
}

fn can_process<C>(slot: U256, contract: &C) -> Result<bool, Box<dyn Error>> 
    where C: SnappContract
{
    let slot_creation_block = contract.creation_block_for_withdraw_slot(slot)?;
	let current_block = contract.get_current_block_number()?;
    Ok(slot_creation_block + 20 < current_block)
}

pub fn run_withdraw_listener<D, C>(db: &D, contract: &C) -> Result<(), Box<dyn Error>> 
    where   D: DbInterface,
            C: SnappContract
{
    let withdraw_slot = contract.get_current_withdraw_slot()?;

    println!("Current top withdraw_slot is {:?}", withdraw_slot);
    if let Some(slot) = find_first_unapplied_slot(withdraw_slot, contract)? {
        println!("Highest unprocessed withdraw_slot is {:?}", slot);
        if can_process(slot, contract)? {
            println!("Processing withdraw_slot is {:?}", slot);
            let state_root = contract.get_current_state_root()?;
            let contract_withdraw_hash = contract.withdraw_hash_for_slot(slot)?;
	        let mut balances = db.get_current_balances(&state_root)?;

            let withdraws = db.get_withdraws_of_slot(slot.low_u32())?;
            // Hash withdraws and compare with contract
            // adjust balances
            // has balances
            contract.apply_withdraws(slot, state_root, _new_state_root, contract_withdraw_hash)?;
        } else {
            println!("Need to wait before withdraw_slot is {:?}", slot);
        }
    }
    Ok(())
}