use crate::models;
use crate::models::RollingHashable;

use crate::db_interface::DbInterface;
use crate::error::{DriverError, ErrorKind};
use crate::contract::SnappContract;
use crate::util::{find_first_unapplied_slot, can_process};


pub fn apply_deposits(
    state: &models::State,
    deposits: &[models::PendingFlux],
) -> models::State {
    let mut updated_state = state.clone();
    for deposit in deposits {
        updated_state.increment_balance(deposit.token_id, deposit.account_id, deposit.amount)
    }
    updated_state
}

pub fn run_deposit_listener<D, C>(db: &D, contract: &C) -> Result<(bool), DriverError> 
    where   D: DbInterface,
            C: SnappContract
{
    let deposit_slot = contract.get_current_deposit_slot()?;

    println!("Current top deposit_slot is {:?}", deposit_slot);
    let slot = find_first_unapplied_slot(
        deposit_slot, 
        Box::new(&|i| contract.has_deposit_slot_been_applied(i))
    )?;
    if slot <= deposit_slot {
        println!("Highest unprocessed deposit_slot is {:?}", slot);
        if can_process(slot, contract,
            Box::new(&|i| contract.creation_timestamp_for_deposit_slot(i))
        )? {
            println!("Processing deposit_slot {:?}", slot);
            let state_root = contract.get_current_state_root()?;
            let contract_deposit_hash = contract.deposit_hash_for_slot(slot)?;
            let balances = db.get_current_balances(&state_root)?;

            let deposits = db.get_deposits_of_slot(slot.low_u32())?;
            let deposit_hash = deposits.rolling_hash(0);
            if deposit_hash != contract_deposit_hash {
                return Err(DriverError::new(
                    &format!("Pending deposit hash from contract ({}), didn't match the one found in db ({})", 
                    contract_deposit_hash, deposit_hash), ErrorKind::StateError
                ));
            }

            let updated_balances = apply_deposits(&balances, &deposits);
            let new_state_root = updated_balances.rolling_hash(balances.state_index + 1);
            
            println!("New State_hash is {}", new_state_root);
            contract.apply_deposits(slot, state_root, new_state_root, contract_deposit_hash)?;
            return Ok(true);
        } else {
            println!("Need to wait before processing deposit_slot {:?}", slot);
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
    use crate::models::flux::tests::create_flux_for_test;
    use crate::db_interface::tests::DbInterfaceMock;
    use mock_it::Matcher::*;
    use web3::types::{H256, U256};

    #[test]
    fn applies_current_state_if_unapplied_and_enough_blocks_passed() {
        let slot = U256::from(1);
        let state_hash = H256::zero();
        let deposits = vec![create_flux_for_test(1,1), create_flux_for_test(1,2)];
        let state = models::State::new(
            format!("{:x}", state_hash),
            1,
            vec![100; (models::TOKENS * 2) as usize],
            models::TOKENS,
        );

        let contract = SnappContractMock::new();
        contract.get_current_deposit_slot.given(()).will_return(Ok(slot));
        contract.has_deposit_slot_been_applied.given(slot).will_return(Ok(false));
        contract.has_deposit_slot_been_applied.given(slot - 1).will_return(Ok(true));
        contract.creation_timestamp_for_deposit_slot.given(slot).will_return(Ok(U256::from(10)));
        contract.get_current_block_timestamp.given(()).will_return(Ok(U256::from(200)));
        contract.deposit_hash_for_slot.given(slot).will_return(Ok(deposits.rolling_hash(0)));
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

        let state = models::State::new(
            format!("{:x}", state_hash),
            1,
            vec![100; (models::TOKENS * 2) as usize],
            models::TOKENS,
        );

        let contract = SnappContractMock::new();
        contract.get_current_state_root.given(()).will_return(Ok(state_hash));        
        contract.get_current_deposit_slot.given(()).will_return(Ok(slot));
        contract.has_deposit_slot_been_applied.given(slot).will_return(Ok(true));

        contract.get_current_block_timestamp.given(()).will_return(Ok(U256::from(11)));
        contract.creation_timestamp_for_deposit_slot.given(slot + 1).will_return(Ok(U256::from(10)));
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

        let state = models::State::new(
            format!("{:x}", state_hash),
            1,
            vec![100; (models::TOKENS * 2) as usize],
            models::TOKENS,
        );

        let contract = SnappContractMock::new();
        contract.get_current_state_root.given(()).will_return(Ok(state_hash));        
        contract.get_current_deposit_slot.given(()).will_return(Ok(slot));
        contract.has_deposit_slot_been_applied.given(slot).will_return(Ok(false));
        contract.has_deposit_slot_been_applied.given(slot-1).will_return(Ok(true));

        contract.creation_timestamp_for_deposit_slot.given(slot).will_return(Ok(U256::from(10)));
        contract.get_current_block_timestamp.given(()).will_return(Ok(U256::from(11)));
        contract.deposit_hash_for_slot.given(slot).will_return(Ok(deposits.rolling_hash(0)));

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

        contract.creation_timestamp_for_deposit_slot.given(slot-1).will_return(Ok(U256::from(10)));
        contract.get_current_block_timestamp.given(()).will_return(Ok(U256::from(200)));
        contract.deposit_hash_for_slot.given(slot-1).will_return(Ok(second_deposits.rolling_hash(0)));

        contract.get_current_state_root.given(()).will_return(Ok(state_hash));
        contract.apply_deposits.given((slot - 1, Any, Any, Any)).will_return(Ok(()));

        let state = models::State::new(
            format!("{:x}", state_hash),
            1,
            vec![100; (models::TOKENS * 2) as usize],
            models::TOKENS,
        );

        let db = DbInterfaceMock::new();
        db.get_deposits_of_slot.given(0).will_return(Ok(first_deposits));
        db.get_current_balances.given(state_hash).will_return(Ok(state));
        
        assert_eq!(run_deposit_listener(&db, &contract), Ok(true));
    }

    #[test]
    fn returns_error_if_db_deposit_hash_doesnt_match_contract() {
        let slot = U256::from(1);
        let state_hash = H256::zero();

        let deposits = vec![create_flux_for_test(1,1), create_flux_for_test(1,2)];

        let state = models::State::new(
            format!("{:x}", state_hash),
            1,
            vec![100; (models::TOKENS * 2) as usize],
            models::TOKENS,
        );

        let contract = SnappContractMock::new();
        contract.get_current_deposit_slot.given(()).will_return(Ok(slot));
        contract.has_deposit_slot_been_applied.given(slot).will_return(Ok(false));
        contract.has_deposit_slot_been_applied.given(slot - 1).will_return(Ok(true));

        contract.creation_timestamp_for_deposit_slot.given(slot).will_return(Ok(U256::from(10)));
        contract.get_current_block_timestamp.given(()).will_return(Ok(U256::from(200)));
        
        contract.deposit_hash_for_slot.given(slot).will_return(Ok(H256::zero()));
        contract.get_current_state_root.given(()).will_return(Ok(state_hash));

        let db = DbInterfaceMock::new();
        db.get_deposits_of_slot.given(1).will_return(Ok(deposits));
        db.get_current_balances.given(state_hash).will_return(Ok(state));

        let error = run_deposit_listener(&db, &contract).expect_err("Expected Error");
        assert_eq!(error.kind, ErrorKind::StateError);
    }
}