use crate::models;

use crate::db_interface::DbInterface;
use crate::contract::SnappContract;
use crate::error::DriverError;

use web3::types::{H256, U256};
use sha2::{Digest, Sha256};
use std::error::Error;

fn apply_withdraws(
    state: &models::State,
    withdraws: &Vec<models::PendingFlux>,
) -> (models::State, Vec<bool>) {
    let mut state = state.clone();
    let mut valid_withdraws = vec![];
    for i in withdraws {
        if state.balances[((i.accountId - 1) * models::TOKENS + (i.tokenId as u16 - 1)) as usize] >= i.amount {
            state.balances[((i.accountId - 1) * models::TOKENS + (i.tokenId as u16 - 1)) as usize] -= i.amount;
            valid_withdraws.push(true);
        } else {
            valid_withdraws.push(false);
        }
    }
    (state, valid_withdraws)
}

fn find_first_unapplied_slot<C>(upper_bound: U256, contract: &C) -> Result<U256, Box<dyn Error>>
    where C: SnappContract
{
    let mut slot = upper_bound;
    while slot != U256::zero() {
        if contract.has_withdraw_slot_been_applied(slot - 1)? {
            return Ok(slot)
        }
        slot = slot - 1;
    }
    Ok(U256::zero())
}

fn can_process<C>(slot: U256, contract: &C) -> Result<bool, Box<dyn Error>> 
    where C: SnappContract
{
    let slot_creation_block = contract.creation_block_for_withdraw_slot(slot)?;
    if slot_creation_block == U256::zero() {
        return Ok( false );
    }
    let current_block = contract.get_current_block_number()?;
    Ok(slot_creation_block + 20 < current_block)
}

fn merkleize(withdraws: Vec<Vec<u8>>) -> H256 {
    if withdraws.len() == 1 {
        return H256::from(withdraws[0].as_slice());
    }
    let next_layer = withdraws.chunks(2).map(|pair| {
        let mut hasher = Sha256::new();
        hasher.input(&pair[0]);
        hasher.input(&pair[1]);
        hasher.result().to_vec()
    }).collect();
    println!("Next layer: {:?}", next_layer);
    merkleize(next_layer)
}

pub fn run_withdraw_listener<D, C>(db: &D, contract: &C) -> Result<(), Box<dyn Error>> 
    where   D: DbInterface,
            C: SnappContract
{
    let withdraw_slot = contract.get_current_withdraw_slot()?;

    println!("Current top withdraw_slot is {:?}", withdraw_slot);
    let slot = find_first_unapplied_slot(withdraw_slot + 1, contract)?;
        println!("Highest unprocessed withdraw_slot is {:?}", slot);
        if can_process(slot, contract)? {
            println!("Processing withdraw_slot {:?}", slot);
            let state_root = contract.get_current_state_root()?;
            let contract_withdraw_hash = contract.withdraw_hash_for_slot(slot)?;
            let balances = db.get_current_balances(&state_root)?;

            let withdraws = db.get_withdraws_of_slot(slot.low_u32())?;
            let withdraw_hash = withdraws.iter().fold(H256::zero(), |acc, w| w.iter_hash(&acc));
            if withdraw_hash != contract_withdraw_hash {
                return Err(Box::new(DriverError::new(
                    &format!("Pending withdraw hash from contract ({}), didn't match the one found in db ({})", 
                    withdraw_hash, contract_withdraw_hash)
                )));
            }

            let (updated_balances, valid_withdraws) = apply_withdraws(&balances, &withdraws);
            
            let mut withdraw_bytes = vec![vec![0; 32]; 128];
            for (index, _) in valid_withdraws.iter().enumerate().filter(|(_, valid)| **valid) {
                withdraw_bytes[index] = withdraws[index].bytes();
            }
            println!("Withdraw Bytes: {:?}", withdraw_bytes);
            let withdrawal_merkle_root = merkleize(withdraw_bytes);
            let new_state_root = H256::from(updated_balances.hash()?);
            
            println!("New StateHash is {}, Valid Withdraw Merkle Root is {}", new_state_root, withdrawal_merkle_root);
            contract.apply_withdraws(slot, withdrawal_merkle_root, state_root, new_state_root, contract_withdraw_hash)?;
        } else {
            println!("Need to wait before processing withdraw_slot {:?}", slot);
        }
    
    Ok(())
}