use crate::db_interface::DbInterface;
use crate::error::DriverError;
use crate::contract::SnappContract;
use crate::util::{find_first_unapplied_slot, can_process, hash_consistency_check};

use dfusion_core::models::{RollingHashable};

pub fn run_deposit_listener<D, C>(db: &D, contract: &C) -> Result<(bool), DriverError> 
    where   D: DbInterface,
            C: SnappContract
{
    let deposit_slot = contract.get_current_deposit_slot()?;

    info!("Current top deposit_slot is {:?}", deposit_slot);
    let slot = find_first_unapplied_slot(
        deposit_slot, 
        &|i| contract.has_deposit_slot_been_applied(i)
    )?;
    if slot <= deposit_slot {
        info!("Highest unprocessed deposit_slot is {:?}", slot);
        if can_process(slot, contract,
            &|i| contract.creation_timestamp_for_deposit_slot(i)
        )? {
            info!("Processing deposit_slot {:?}", slot);
            let state_root = contract.get_current_state_root()?;
            let contract_deposit_hash = contract.deposit_hash_for_slot(slot)?;
            let mut balances = db.get_current_balances(&state_root)?;

            let deposits = db.get_deposits_of_slot(slot.low_u32())?;
            let deposit_hash = deposits.rolling_hash(0);
            hash_consistency_check(deposit_hash, contract_deposit_hash, "deposit")?;

            balances.apply_deposits(&deposits);
            
            info!("New AccountState hash is {}", balances.state_hash);
            contract.apply_deposits(slot, state_root, balances.state_hash, contract_deposit_hash)?;
            return Ok(true);
        } else {
            info!("Need to wait before processing deposit_slot {:?}", slot);
        }
    } else {
        info!("All deposits are already processed");
    }
    Ok(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use dfusion_core::models::flux::tests::create_flux_for_test;
    use dfusion_core::models;
    use crate::contract::tests::SnappContractMock;
    use crate::db_interface::tests::DbInterfaceMock;
    use mock_it::Matcher::*;
    use web3::types::{H256, U256};
    use crate::error::{ErrorKind};

    #[test]
    fn applies_current_state_if_unapplied_and_enough_blocks_passed() {
        let slot = U256::from(1);
        let state_hash = H256::zero();
        let deposits = vec![create_flux_for_test(1,1), create_flux_for_test(1,2)];
        let state = models::AccountState::new(
            state_hash,
            U256::one(),
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

        let state = models::AccountState::new(
            state_hash,
            U256::one(),
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

        let state = models::AccountState::new(
            state_hash,
            U256::one(),
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

        let state = models::AccountState::new(
            state_hash,
            U256::one(),
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

        let state = models::AccountState::new(
            state_hash,
            U256::one(),
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