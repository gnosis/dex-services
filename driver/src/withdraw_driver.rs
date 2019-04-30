use crate::models;
use crate::models::{RollingHashable, RootHashable};

use crate::db_interface::DbInterface;
use crate::contract::SnappContract;
use crate::error::{DriverError, ErrorKind};
use crate::util;

fn apply_withdraws(
    state: &models::State,
    withdraws: &[models::PendingFlux],
) -> (models::State, Vec<bool>) {
    let mut state = state.clone();  // TODO - does this really need to be cloned
    let mut valid_withdraws = vec![];
    for w in withdraws {
        if state.read_balance(w.token_id, w.account_id) >= w.amount {
            state.decrement_balance(w.token_id, w.account_id, w.amount);
            valid_withdraws.push(true);
        } else {
            valid_withdraws.push(false);
        }
    }
    (state, valid_withdraws)
}

pub fn run_withdraw_listener<D, C>(db: &D, contract: &C) -> Result<(bool), DriverError> 
    where   D: DbInterface,
            C: SnappContract
{
    let withdraw_slot = contract.get_current_withdraw_slot()?;

    println!("Current top withdraw_slot is {:?}", withdraw_slot);
    let slot = util::find_first_unapplied_slot(
        withdraw_slot, 
        Box::new(&|i| contract.has_withdraw_slot_been_applied(i))
    )?;
    if slot <= withdraw_slot {
        println!("Highest unprocessed withdraw_slot is {:?}", slot);
        if util::can_process(slot, contract,
            Box::new(&|i| contract.creation_timestamp_for_withdraw_slot(i))
        )? {
            println!("Processing withdraw_slot {:?}", slot);
            let state_root = contract.get_current_state_root()?;
            let contract_withdraw_hash = contract.withdraw_hash_for_slot(slot)?;
            let balances = db.get_current_balances(&state_root)?;

            let withdraws = db.get_withdraws_of_slot(slot.low_u32())?;
            let withdraw_hash = withdraws.rolling_hash(0);
            if withdraw_hash != contract_withdraw_hash {
                return Err(DriverError::new(
                    &format!("Pending withdraw hash from contract ({}), didn't match the one found in db ({})", 
                    withdraw_hash, contract_withdraw_hash), ErrorKind::StateError
                ));
            }

            let (updated_balances, valid_withdraws) = apply_withdraws(&balances, &withdraws);
            let withdrawal_merkle_root = withdraws.root_hash(&valid_withdraws);
            let new_state_root = updated_balances.rolling_hash(balances.state_index + 1);
            
            println!("New State_hash is {}, Valid Withdraw Merkle Root is {}", new_state_root, withdrawal_merkle_root);
            contract.apply_withdraws(slot, withdrawal_merkle_root, state_root, new_state_root, contract_withdraw_hash)?;
            return Ok(true);
        } else {
            println!("Need to wait before processing withdraw_slot {:?}", slot);
        }
    }
    Ok(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::contract::tests::SnappContractMock;
    use crate::models::flux::tests::create_flux_for_test;
    use crate::db_interface::tests::DbInterfaceMock;
    use mock_it::Matcher::*;
    use web3::types::{H256, U256};

    #[test]
    fn applies_current_state_if_unapplied_and_enough_blocks_passed() {
        let slot = U256::from(1);
        let state_hash = H256::zero();
        let withdraws = vec![create_flux_for_test(1,1), create_flux_for_test(1,2)];
        let state = models::State::new(
            format!("{:x}", state_hash),
            1,
            vec![100; (models::TOKENS * 2) as usize],
            models::TOKENS,
        );

        let contract = SnappContractMock::new();
        contract.get_current_withdraw_slot.given(()).will_return(Ok(slot));
        contract.has_withdraw_slot_been_applied.given(slot).will_return(Ok(false));
        contract.has_withdraw_slot_been_applied.given(slot - 1).will_return(Ok(true));
        contract.creation_timestamp_for_withdraw_slot.given(slot).will_return(Ok(U256::from(10)));
        contract.get_current_block_timestamp.given(()).will_return(Ok(U256::from(200)));
        contract.withdraw_hash_for_slot.given(slot).will_return(Ok(withdraws.rolling_hash(0)));
        contract.get_current_state_root.given(()).will_return(Ok(state_hash));
        contract.apply_withdraws.given((slot, Any, Any, Any, Any)).will_return(Ok(()));

        let db = DbInterfaceMock::new();
        db.get_withdraws_of_slot.given(1).will_return(Ok(withdraws));
        db.get_current_balances.given(state_hash).will_return(Ok(state));

        assert_eq!(run_withdraw_listener(&db, &contract), Ok(true));
    }

    #[test]
    fn does_not_apply_if_highest_slot_already_applied() {
        let slot = U256::from(1);
        let contract = SnappContractMock::new();
        contract.get_current_withdraw_slot.given(()).will_return(Ok(slot));
        contract.has_withdraw_slot_been_applied.given(slot).will_return(Ok(true));

        let db = DbInterfaceMock::new();
        assert_eq!(run_withdraw_listener(&db, &contract), Ok(false));
    }

    #[test]
    fn does_not_apply_if_highest_slot_too_close_to_current_block() {
        let slot = U256::from(1);
        let contract = SnappContractMock::new();
        contract.get_current_withdraw_slot.given(()).will_return(Ok(slot));
        contract.has_withdraw_slot_been_applied.given(slot).will_return(Ok(false));
        contract.has_withdraw_slot_been_applied.given(slot-1).will_return(Ok(true));

        contract.creation_timestamp_for_withdraw_slot.given(slot).will_return(Ok(U256::from(10)));
        contract.get_current_block_timestamp.given(()).will_return(Ok(U256::from(11)));

        let db = DbInterfaceMock::new();
        assert_eq!(run_withdraw_listener(&db, &contract), Ok(false));
    }

    #[test]
    fn applies_all_unapplied_states_before_current() {
        let slot = U256::from(1);
        let state_hash = H256::zero();
        let first_withdraws = vec![create_flux_for_test(0,1), create_flux_for_test(0,2)];
        let second_withdraws = vec![create_flux_for_test(1,1), create_flux_for_test(1,2)];

        let contract = SnappContractMock::new();
        contract.get_current_withdraw_slot.given(()).will_return(Ok(slot));

        contract.has_withdraw_slot_been_applied.given(slot).will_return(Ok(false));
        contract.has_withdraw_slot_been_applied.given(slot - 1).will_return(Ok(false));

        contract.creation_timestamp_for_withdraw_slot.given(slot-1).will_return(Ok(U256::from(10)));

        contract.get_current_block_timestamp.given(()).will_return(Ok(U256::from(200)));
        contract.withdraw_hash_for_slot.given(slot-1).will_return(Ok(second_withdraws.rolling_hash(0)));

        contract.get_current_state_root.given(()).will_return(Ok(state_hash));
        contract.apply_withdraws.given((slot - 1, Any, Any, Any, Any)).will_return(Ok(()));

        let state = models::State::new(
            format!("{:x}", state_hash),
            1,
            vec![100; (models::TOKENS * 2) as usize],
            models::TOKENS,
        );

        let db = DbInterfaceMock::new();
        db.get_withdraws_of_slot.given(0).will_return(Ok(first_withdraws));
        db.get_current_balances.given(state_hash).will_return(Ok(state));
        
        assert_eq!(run_withdraw_listener(&db, &contract), Ok(true));
        assert_eq!(run_withdraw_listener(&db, &contract), Ok(true));
    }

    #[test]
    fn returns_error_if_db_withdraw_hash_doesnt_match_cotract() {
        let slot = U256::from(1);
        let state_hash = H256::zero();

        let withdraws = vec![create_flux_for_test(1,1), create_flux_for_test(1,2)];

        let state = models::State::new(
            format!("{:x}", state_hash),
            1,
            vec![100; (models::TOKENS * 2) as usize],
            models::TOKENS,
        );

        let contract = SnappContractMock::new();
        contract.get_current_withdraw_slot.given(()).will_return(Ok(slot));
        contract.has_withdraw_slot_been_applied.given(slot).will_return(Ok(false));
        contract.has_withdraw_slot_been_applied.given(slot - 1).will_return(Ok(true));

        contract.creation_timestamp_for_withdraw_slot.given(slot).will_return(Ok(U256::from(10)));
        contract.get_current_block_timestamp.given(()).will_return(Ok(U256::from(200)));
        
        contract.withdraw_hash_for_slot.given(slot).will_return(Ok(H256::zero()));
        contract.get_current_state_root.given(()).will_return(Ok(state_hash));

        let db = DbInterfaceMock::new();
        db.get_withdraws_of_slot.given(1).will_return(Ok(withdraws));
        db.get_current_balances.given(state_hash).will_return(Ok(state));

        let error = run_withdraw_listener(&db, &contract).expect_err("Expected Error");
        assert_eq!(error.kind, ErrorKind::StateError);
    }

    #[test]
    fn skips_invalid_balances_in_applied_merkle_tree() {
        let slot = U256::from(1);
        let state_hash = H256::zero();
        let withdraws = vec![create_flux_for_test(1,1), models::PendingFlux {
            slot_index: 2,
            slot: 1,
            account_id: 0,
            token_id: 1,
            amount: 10,
        }];
        let mut state = models::State::new(
            format!("{:x}", state_hash),
            1,
            vec![100; (models::TOKENS * 2) as usize],
            models::TOKENS,
        );
        state.decrement_balance(1, 0, 100);

        let merkle_root = withdraws.root_hash(&vec![true, false]);

        let contract = SnappContractMock::new();
        contract.get_current_withdraw_slot.given(()).will_return(Ok(slot));
        contract.has_withdraw_slot_been_applied.given(slot).will_return(Ok(false));
        contract.has_withdraw_slot_been_applied.given(slot - 1).will_return(Ok(true));
        contract.creation_timestamp_for_withdraw_slot.given(slot).will_return(Ok(U256::from(10)));
        contract.get_current_block_timestamp.given(()).will_return(Ok(U256::from(200)));
        contract.withdraw_hash_for_slot.given(slot).will_return(Ok(withdraws.rolling_hash(0)));
        contract.get_current_state_root.given(()).will_return(Ok(state_hash));
        contract.apply_withdraws.given((slot, Val(merkle_root), Any, Any, Any)).will_return(Ok(()));

        let db = DbInterfaceMock::new();
        db.get_withdraws_of_slot.given(1).will_return(Ok(withdraws));
        db.get_current_balances.given(state_hash).will_return(Ok(state));

        assert_eq!(run_withdraw_listener(&db, &contract), Ok(true));
    }
}