use crate::models;
use crate::db_interface::DbInterface;
use crate::contract::SnappContract;
use crate::error::DriverError;

use web3::types::{H256, U256};

use std::error::Error;

pub fn apply_deposits(
    state: &mut models::State,
    deposits: &Vec<models::PendingFlux>,
) -> models::State {
    for i in deposits {
        state.balances[((i.accountId - 1) * models::TOKENS + (i.tokenId as u16 - 1)) as usize] += i.amount;
    }
    state.clone()
}

pub fn run_deposit_listener<D, C>(db: &D, contract: &C) -> Result<(), Box<dyn Error>> 
    where   D: DbInterface,
            C: SnappContract
{
    let curr_state_root: H256 = contract.get_current_state_root()?;
    let mut state = db.get_current_balances(&curr_state_root)?;

    // check that operator has sufficient ether
    
    let current_deposit_ind: U256 = contract.get_current_deposit_slot()?;

    // get latest non-applied deposit_index
    let mut deposit_ind = current_deposit_ind + 1;
    println!("Current top deposit_slot is {:?}", deposit_ind);
    let mut found: bool = false;

    // Starting from the last depositSlot, we search backwards to the first non-applied deposit
    while !found && deposit_ind != U256::zero() {
        deposit_ind = deposit_ind - 1;
        found = contract.has_deposit_slot_been_applied(deposit_ind)?;
    }
    if found {
        deposit_ind = deposit_ind + 1;
    }
    println!("Current pending deposit_slot is {:?}", deposit_ind);

    let current_deposit_ind_block = contract.creation_block_for_deposit_slot(deposit_ind)?;
    let current_block = contract.get_current_block_number()?;

    let deposit_hash_pulled: H256 = contract.deposit_hash_for_slot(deposit_ind)?;
    let deposit_slot_empty = deposit_hash_pulled == H256::zero();

    println!(
        "Current block is {:?} and the last deposit_ind_creationBlock is {:?}",
        current_block, current_deposit_ind_block
    );

    // if 20 blocks have past since the first deposit and we are not in the newest slot, we apply the deposit.
    if current_deposit_ind_block + 20 < current_block
        && deposit_ind != current_deposit_ind + 1
    {
        println!("Next deposit_slot to be processed is {}", deposit_ind);
        let deposits = db.get_deposits_of_slot(deposit_ind.low_u32())?;
        println!("Amount of deposits to be processed{:?}", deposits.len());
        //rehash deposits
        let mut deposit_hash: H256 = H256::zero();
        for pat in &deposits {
            deposit_hash = pat.iter_hash(&mut deposit_hash)
        }

        if deposit_hash != deposit_hash_pulled {
            return Err(Box::new(DriverError::new(
                &format!("Pending deposit hash from contract ({}), didn't match the one found in db ({})", 
                deposit_hash, deposit_hash_pulled)
            )));
        }

        if deposit_slot_empty && deposit_ind != U256::zero() {
            println!("deposit_slot {} already processed", deposit_ind);
        } else {
            // calculate new state by applying all deposits
            state = apply_deposits(&mut state, &deposits);
            println!("New StateHash is{:?}", state.hash()?);

            //send new state into blockchain
            //applyDeposits signature is (slot, _currStateRoot, _newStateRoot, deposit_slotHash)
            let slot = U256::from(deposit_ind);
            let _curr_state_root = curr_state_root;
            let _new_state_root = H256::from(state.hash()?);

            contract.apply_deposits(slot, _curr_state_root, _new_state_root, deposit_hash_pulled)?;
        }
    } else {
        println!("All deposits are already processed");
    }
    Ok(())
}
