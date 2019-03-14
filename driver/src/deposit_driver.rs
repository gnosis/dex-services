use crate::models;
use crate::models::RollingHashable;

use crate::db_interface::DbInterface;
use crate::error::{DriverError, ErrorKind};
use crate::contract::SnappContract;

use web3::types::{H256, U256};

pub fn apply_deposits(
    state: &mut models::State,
    deposits: &Vec<models::PendingFlux>,
) -> models::State {
    for i in deposits {
        state.balances[((i.account_id - 1) * models::TOKENS + (i.token_id as u16 - 1)) as usize] += i.amount;
    }
    state.clone()
}

pub fn run_deposit_listener<D, C>(db: &D, contract: &C) -> Result<(bool), DriverError> 
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
            return Err(DriverError::new(
                &format!("Pending deposit hash from contract ({}), didn't match the one found in db ({})", 
                deposit_hash, deposit_hash_pulled), ErrorKind::StateError
            ));
        }

        if deposit_slot_empty && deposit_ind != U256::zero() {
            println!("deposit_slot {} already processed", deposit_ind);
        } else {
            // calculate new state by applying all deposits
            state = apply_deposits(&mut state, &deposits);
            println!("New State_hash is{:?}", state.rolling_hash());

            //send new state into blockchain
            //applyDeposits signature is (slot, _currStateRoot, _newStateRoot, deposit_slotHash)
            let slot = U256::from(deposit_ind);
            let _curr_state_root = curr_state_root;
            let _new_state_root = H256::from(state.rolling_hash());

            contract.apply_deposits(slot, _curr_state_root, _new_state_root, deposit_hash_pulled)?;
            return Ok(true);
        }
    } else {
        println!("All deposits are already processed");
    }
    Ok(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::RollingHashable;
    use crate::contract::tests::SnappContractMock;
    use crate::models::tests::create_flux_for_test;
    use crate::db_interface::tests::DbInterfaceMock;
    use mock_it::Matcher::*;

    #[test]
    fn applies_current_state_if_unapplied_and_enough_blocks_passed() {
        let slot = U256::from(1);
        let state_hash = H256::zero();
        let deposits = vec![create_flux_for_test(1,1), create_flux_for_test(1,2)];
        let state = models::State {
            state_hash: format!("{:x}", state_hash),
            state_index: 1,
            balances: vec![100; (models::TOKENS * 2) as usize],
        };

        let contract = SnappContractMock::new();
        contract.get_current_deposit_slot.given(()).will_return(Ok(slot));
        contract.has_deposit_slot_been_applied.given(slot).will_return(Ok(false));
        contract.has_deposit_slot_been_applied.given(slot - 1).will_return(Ok(true));
        contract.creation_block_for_deposit_slot.given(slot).will_return(Ok(U256::from(10)));
        contract.get_current_block_number.given(()).will_return(Ok(U256::from(34)));
        contract.deposit_hash_for_slot.given(slot).will_return(Ok(deposits.rolling_hash()));
        contract.get_current_state_root.given(()).will_return(Ok(state_hash));
        contract.apply_deposits.given((slot, Any, Any, Any)).will_return(Ok(()));

        let db = DbInterfaceMock::new();
        db.get_deposits_of_slot.given(1).will_return(Ok(deposits));
        db.get_current_balances.given(state_hash).will_return(Ok(state));

        assert_eq!(run_deposit_listener(&db, &contract), Ok(true));
    }

    #[test]
    fn does_not_apply_if_highest_slot_already_applied() {
        let slot = U256::from(1);
        let state_hash = H256::zero();

        let state = models::State {
            state_hash: format!("{:x}", state_hash),
            state_index: 1,
            balances: vec![100; (models::TOKENS * 2) as usize],
        };

        let contract = SnappContractMock::new();
        contract.get_current_state_root.given(()).will_return(Ok(state_hash));        
        contract.get_current_deposit_slot.given(()).will_return(Ok(slot));
        contract.has_deposit_slot_been_applied.given(slot).will_return(Ok(true));

        contract.get_current_block_number.given(()).will_return(Ok(U256::from(11)));
        contract.creation_block_for_deposit_slot.given(slot + 1).will_return(Ok(U256::from(10)));
        contract.deposit_hash_for_slot.given(slot + 1).will_return(Ok(H256::zero()));

        let db = DbInterfaceMock::new();
        db.get_current_balances.given(state_hash).will_return(Ok(state));

        assert_eq!(run_deposit_listener(&db, &contract), Ok(false));
    }

    #[test]
    fn does_not_apply_if_highest_slot_too_close_to_current_block() {
        let slot = U256::from(1);
        let state_hash = H256::zero();
        let deposits = vec![create_flux_for_test(1,1), create_flux_for_test(1,2)];

        let state = models::State {
            state_hash: format!("{:x}", state_hash),
            state_index: 1,
            balances: vec![100; (models::TOKENS * 2) as usize],
        };

        let contract = SnappContractMock::new();
        contract.get_current_state_root.given(()).will_return(Ok(state_hash));        
        contract.get_current_deposit_slot.given(()).will_return(Ok(slot));
        contract.has_deposit_slot_been_applied.given(slot).will_return(Ok(false));
        contract.has_deposit_slot_been_applied.given(slot-1).will_return(Ok(true));

        contract.creation_block_for_deposit_slot.given(slot).will_return(Ok(U256::from(10)));
        contract.get_current_block_number.given(()).will_return(Ok(U256::from(11)));
        contract.deposit_hash_for_slot.given(slot).will_return(Ok(deposits.rolling_hash()));

        let db = DbInterfaceMock::new();
        db.get_current_balances.given(state_hash).will_return(Ok(state));

        assert_eq!(run_deposit_listener(&db, &contract), Ok(false));
    }

    #[test]
    fn applies_all_unapplied_states_before_current() {
        let slot = U256::from(1);
        let state_hash = H256::zero();
        let first_deposits = vec![create_flux_for_test(0,1), create_flux_for_test(0,2)];
        let second_deposits = vec![create_flux_for_test(1,1), create_flux_for_test(1,2)];

        let contract = SnappContractMock::new();
        contract.get_current_deposit_slot.given(()).will_return(Ok(slot));

        contract.has_deposit_slot_been_applied.given(slot).will_return(Ok(false));
        contract.has_deposit_slot_been_applied.given(slot - 1).will_return(Ok(false));

        contract.creation_block_for_deposit_slot.given(slot-1).will_return(Ok(U256::from(10)));
        contract.get_current_block_number.given(()).will_return(Ok(U256::from(34)));
        contract.deposit_hash_for_slot.given(slot-1).will_return(Ok(second_deposits.rolling_hash()));

        contract.get_current_state_root.given(()).will_return(Ok(state_hash));
        contract.apply_deposits.given((slot - 1, Any, Any, Any)).will_return(Ok(()));

        let state = models::State {
            state_hash: format!("{:x}", state_hash),
            state_index: 1,
            balances: vec![100; (models::TOKENS * 2) as usize],
        };

        let db = DbInterfaceMock::new();
        db.get_deposits_of_slot.given(0).will_return(Ok(first_deposits));
        db.get_current_balances.given(state_hash).will_return(Ok(state));
        
        assert_eq!(run_deposit_listener(&db, &contract), Ok(true));
    }

    #[test]
    fn returns_error_if_db_deposit_hash_doesnt_match_cotract() {
        let slot = U256::from(1);
        let state_hash = H256::zero();

        let deposits = vec![create_flux_for_test(1,1), create_flux_for_test(1,2)];

        let state = models::State {
            state_hash: format!("{:x}", state_hash),
            state_index: 1,
            balances: vec![100; (models::TOKENS * 2) as usize],
        };

        let contract = SnappContractMock::new();
        contract.get_current_deposit_slot.given(()).will_return(Ok(slot));
        contract.has_deposit_slot_been_applied.given(slot).will_return(Ok(false));
        contract.has_deposit_slot_been_applied.given(slot - 1).will_return(Ok(true));

        contract.creation_block_for_deposit_slot.given(slot).will_return(Ok(U256::from(10)));
        contract.get_current_block_number.given(()).will_return(Ok(U256::from(34)));
        
        contract.deposit_hash_for_slot.given(slot).will_return(Ok(H256::zero()));
        contract.get_current_state_root.given(()).will_return(Ok(state_hash));

        let db = DbInterfaceMock::new();
        db.get_deposits_of_slot.given(1).will_return(Ok(deposits));
        db.get_current_balances.given(state_hash).will_return(Ok(state));

        let error = run_deposit_listener(&db, &contract).expect_err("Expected Error");
        assert_eq!(error.kind, ErrorKind::StateError);
    }
}