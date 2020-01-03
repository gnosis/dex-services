use crate::contracts::snapp_contract::SnappContract;
use crate::error::DriverError;
use crate::util::{
    batch_processing_state, find_first_unapplied_slot, hash_consistency_check, ProcessingState,
};

use dfusion_core::database::DbInterface;
use dfusion_core::models::{RollingHashable, RootHashable};

use log::info;

pub fn run_withdraw_listener(
    db: &dyn DbInterface,
    contract: &dyn SnappContract,
) -> Result<bool, DriverError> {
    let withdraw_slot = contract.get_current_withdraw_slot()?;

    info!("Current top withdraw_slot is {:?}", withdraw_slot);
    let slot = find_first_unapplied_slot(withdraw_slot, &|i| {
        contract.has_withdraw_slot_been_applied(i)
    })?;
    if slot <= withdraw_slot {
        info!("Highest unprocessed withdraw_slot is {:?}", slot);
        let processing_state = batch_processing_state(slot, contract, &|i| {
            contract.creation_timestamp_for_withdraw_slot(i)
        })?;
        match processing_state {
            ProcessingState::TooEarly => {
                info!("Need to wait before processing withdraw_slot {:?}", slot)
            }
            ProcessingState::AcceptsBids | ProcessingState::AcceptsSolution => {
                info!("Processing withdraw_slot {:?}", slot);
                let state_root = contract.get_current_state_root()?;
                let contract_withdraw_hash = contract.withdraw_hash_for_slot(slot)?;
                let mut balances = db.get_balances_for_state_root(&state_root)?;

                let withdraws = db.get_withdraws_of_slot(&slot)?;
                let withdraw_hash = withdraws.rolling_hash(0);
                hash_consistency_check(withdraw_hash, contract_withdraw_hash, "withdraw")?;

                let valid_withdraws = balances.apply_withdraws(&withdraws);
                let withdrawal_merkle_root = withdraws.root_hash(&valid_withdraws);

                info!(
                    "New AccountState hash is {}, Valid Withdraw Merkle Root is {}",
                    balances.state_hash, withdrawal_merkle_root
                );
                contract.apply_withdraws(
                    slot,
                    withdrawal_merkle_root,
                    state_root,
                    balances.state_hash,
                    contract_withdraw_hash,
                )?;
                return Ok(true);
            }
        }
    }
    Ok(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::contracts::snapp_contract::tests::SnappContractMock;
    use crate::error::ErrorKind;
    use dfusion_core::database::tests::DbInterfaceMock;
    use dfusion_core::models::flux::tests::create_flux_for_test;
    use dfusion_core::models::{AccountState, PendingFlux};
    use mock_it::Matcher::*;
    use web3::types::{H160, H256, U256};

    const NUM_TOKENS: u16 = 10;
    const BALANCES: Vec<u128> = vec![100; (NUM_TOKENS * 2) as usize];

    #[test]
    fn applies_current_state_if_unapplied_and_enough_blocks_passed() {
        let slot = U256::from(1);
        let state_hash = H256::zero();
        let withdraws = vec![create_flux_for_test(1, 1), create_flux_for_test(1, 2)];
        let state = AccountState::new(
            state_hash,
            U256::one(),
            BALANCES,
            NUM_TOKENS,
        );

        let contract = SnappContractMock::default();
        contract
            .get_current_withdraw_slot
            .given(())
            .will_return(Ok(slot));
        contract
            .has_withdraw_slot_been_applied
            .given(slot)
            .will_return(Ok(false));
        contract
            .has_withdraw_slot_been_applied
            .given(slot - 1)
            .will_return(Ok(true));
        contract
            .creation_timestamp_for_withdraw_slot
            .given(slot)
            .will_return(Ok(U256::from(10)));
        contract
            .get_current_block_timestamp
            .given(())
            .will_return(Ok(U256::from(200)));
        contract
            .withdraw_hash_for_slot
            .given(slot)
            .will_return(Ok(withdraws.rolling_hash(0)));
        contract
            .get_current_state_root
            .given(())
            .will_return(Ok(state_hash));
        contract
            .apply_withdraws
            .given((slot, Any, Any, Any, Any))
            .will_return(Ok(()));

        let db = DbInterfaceMock::new();
        db.get_withdraws_of_slot
            .given(U256::one())
            .will_return(Ok(withdraws));
        db.get_balances_for_state_root
            .given(state_hash)
            .will_return(Ok(state));

        assert_eq!(run_withdraw_listener(&db, &contract), Ok(true));
    }

    #[test]
    fn does_not_apply_if_highest_slot_already_applied() {
        let slot = U256::from(1);
        let contract = SnappContractMock::default();
        contract
            .get_current_withdraw_slot
            .given(())
            .will_return(Ok(slot));
        contract
            .has_withdraw_slot_been_applied
            .given(slot)
            .will_return(Ok(true));

        let db = DbInterfaceMock::new();
        assert_eq!(run_withdraw_listener(&db, &contract), Ok(false));
    }

    #[test]
    fn does_not_apply_if_highest_slot_too_close_to_current_block() {
        let slot = U256::from(1);
        let contract = SnappContractMock::default();
        contract
            .get_current_withdraw_slot
            .given(())
            .will_return(Ok(slot));
        contract
            .has_withdraw_slot_been_applied
            .given(slot)
            .will_return(Ok(false));
        contract
            .has_withdraw_slot_been_applied
            .given(slot - 1)
            .will_return(Ok(true));

        contract
            .creation_timestamp_for_withdraw_slot
            .given(slot)
            .will_return(Ok(U256::from(10)));
        contract
            .get_current_block_timestamp
            .given(())
            .will_return(Ok(U256::from(11)));

        let db = DbInterfaceMock::new();
        assert_eq!(run_withdraw_listener(&db, &contract), Ok(false));
    }

    #[test]
    fn applies_all_unapplied_states_before_current() {
        let slot = U256::from(1);
        let state_hash = H256::zero();
        let first_withdraws = vec![create_flux_for_test(0, 1), create_flux_for_test(0, 2)];
        let second_withdraws = vec![create_flux_for_test(1, 1), create_flux_for_test(1, 2)];

        let contract = SnappContractMock::default();
        contract
            .get_current_withdraw_slot
            .given(())
            .will_return(Ok(slot));

        contract
            .has_withdraw_slot_been_applied
            .given(slot)
            .will_return(Ok(false));
        contract
            .has_withdraw_slot_been_applied
            .given(slot - 1)
            .will_return(Ok(false));

        contract
            .creation_timestamp_for_withdraw_slot
            .given(slot - 1)
            .will_return(Ok(U256::from(10)));

        contract
            .get_current_block_timestamp
            .given(())
            .will_return(Ok(U256::from(200)));
        contract
            .withdraw_hash_for_slot
            .given(slot - 1)
            .will_return(Ok(second_withdraws.rolling_hash(0)));

        contract
            .get_current_state_root
            .given(())
            .will_return(Ok(state_hash));
        contract
            .apply_withdraws
            .given((slot - 1, Any, Any, Any, Any))
            .will_return(Ok(()));

        let state = AccountState::new(
            state_hash,
            U256::one(),
            BALANCES,
            NUM_TOKENS,
        );

        let db = DbInterfaceMock::new();
        db.get_withdraws_of_slot
            .given(U256::zero())
            .will_return(Ok(first_withdraws));
        db.get_balances_for_state_root
            .given(state_hash)
            .will_return(Ok(state));

        assert_eq!(run_withdraw_listener(&db, &contract), Ok(true));
        assert_eq!(run_withdraw_listener(&db, &contract), Ok(true));
    }

    #[test]
    fn returns_error_if_db_withdraw_hash_doesnt_match_cotract() {
        let slot = U256::from(1);
        let state_hash = H256::zero();

        let withdraws = vec![create_flux_for_test(1, 1), create_flux_for_test(1, 2)];

        let state = AccountState::new(
            state_hash,
            U256::one(),
            BALANCES,
            NUM_TOKENS,
        );

        let contract = SnappContractMock::default();
        contract
            .get_current_withdraw_slot
            .given(())
            .will_return(Ok(slot));
        contract
            .has_withdraw_slot_been_applied
            .given(slot)
            .will_return(Ok(false));
        contract
            .has_withdraw_slot_been_applied
            .given(slot - 1)
            .will_return(Ok(true));

        contract
            .creation_timestamp_for_withdraw_slot
            .given(slot)
            .will_return(Ok(U256::from(10)));
        contract
            .get_current_block_timestamp
            .given(())
            .will_return(Ok(U256::from(200)));

        contract
            .withdraw_hash_for_slot
            .given(slot)
            .will_return(Ok(H256::zero()));
        contract
            .get_current_state_root
            .given(())
            .will_return(Ok(state_hash));

        let db = DbInterfaceMock::new();
        db.get_withdraws_of_slot
            .given(U256::one())
            .will_return(Ok(withdraws));
        db.get_balances_for_state_root
            .given(state_hash)
            .will_return(Ok(state));

        let error = run_withdraw_listener(&db, &contract).expect_err("Expected Error");
        assert_eq!(error.kind, ErrorKind::StateError);
    }

    #[test]
    fn skips_invalid_balances_in_applied_merkle_tree() {
        let slot = U256::from(1);
        let state_hash = H256::zero();
        let withdraws = vec![
            create_flux_for_test(1, 1),
            PendingFlux {
                slot_index: 2,
                slot: U256::one(),
                account_id: H160::from_low_u64_be(0),
                token_id: 1,
                amount: 10,
            },
        ];
        let mut state = AccountState::new(
            state_hash,
            U256::one(),
            BALANCES,
            NUM_TOKENS,
        );
        state.decrement_balance(1, H160::from_low_u64_be(0), 100);

        let merkle_root = withdraws.root_hash(&[true, false]);

        let contract = SnappContractMock::default();
        contract
            .get_current_withdraw_slot
            .given(())
            .will_return(Ok(slot));
        contract
            .has_withdraw_slot_been_applied
            .given(slot)
            .will_return(Ok(false));
        contract
            .has_withdraw_slot_been_applied
            .given(slot - 1)
            .will_return(Ok(true));
        contract
            .creation_timestamp_for_withdraw_slot
            .given(slot)
            .will_return(Ok(U256::from(10)));
        contract
            .get_current_block_timestamp
            .given(())
            .will_return(Ok(U256::from(200)));
        contract
            .withdraw_hash_for_slot
            .given(slot)
            .will_return(Ok(withdraws.rolling_hash(0)));
        contract
            .get_current_state_root
            .given(())
            .will_return(Ok(state_hash));
        contract
            .apply_withdraws
            .given((slot, Val(merkle_root), Any, Any, Any))
            .will_return(Ok(()));

        let db = DbInterfaceMock::new();
        db.get_withdraws_of_slot
            .given(U256::one())
            .will_return(Ok(withdraws));
        db.get_balances_for_state_root
            .given(state_hash)
            .will_return(Ok(state));

        assert_eq!(run_withdraw_listener(&db, &contract), Ok(true));
    }
}
