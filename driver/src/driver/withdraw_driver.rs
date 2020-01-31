use crate::contracts::snapp_contract::SnappContract;
use crate::error::DriverError;
use crate::util::{
    batch_processing_state, find_first_unapplied_slot, hash_consistency_check, ProcessingState,
};

use dfusion_core::database::DbInterface;
use dfusion_core::models::{RollingHashable, RootHashable};

use log::{debug, info};

pub fn run_withdraw_listener(
    db: &dyn DbInterface,
    contract: &dyn SnappContract,
) -> Result<bool, DriverError> {
    let withdraw_slot = contract.get_current_withdraw_slot()?;

    debug!("Current top withdraw_slot is {:?}", withdraw_slot);
    let slot = find_first_unapplied_slot(withdraw_slot, &|i| {
        contract.has_withdraw_slot_been_applied(i)
    })?;
    if slot <= withdraw_slot {
        debug!("Highest unprocessed withdraw_slot is {:?}", slot);
        let processing_state = batch_processing_state(slot, contract, &|i| {
            contract.creation_timestamp_for_withdraw_slot(i)
        })?;
        match processing_state {
            ProcessingState::TooEarly => {
                debug!("Need to wait before processing withdraw_slot {:?}", slot)
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
                    withdraw_hash,
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
    use crate::contracts::snapp_contract::MockSnappContract;
    use crate::error::ErrorKind;
    use dfusion_core::database::MockDbInterface;
    use dfusion_core::models::flux::tests::create_flux_for_test;
    use dfusion_core::models::{AccountState, PendingFlux};
    use mockall::predicate::*;
    use web3::types::{H160, H256, U256};

    const NUM_TOKENS: u16 = 30;

    #[test]
    fn applies_current_state_if_unapplied_and_enough_blocks_passed() {
        let slot = U256::from(1);
        let state_hash = H256::zero();
        let withdraws = vec![create_flux_for_test(1, 1), create_flux_for_test(1, 2)];
        let state = AccountState::new(
            state_hash,
            U256::one(),
            vec![100; (NUM_TOKENS * 2) as usize],
            NUM_TOKENS,
        );

        let mut contract = MockSnappContract::default();
        contract
            .expect_get_current_withdraw_slot()
            .return_const(Ok(slot));
        contract
            .expect_has_withdraw_slot_been_applied()
            .with(eq(slot))
            .return_const(Ok(false));
        contract
            .expect_has_withdraw_slot_been_applied()
            .with(eq(slot - 1))
            .return_const(Ok(true));
        contract
            .expect_creation_timestamp_for_withdraw_slot()
            .with(eq(slot))
            .return_const(Ok(U256::from(10)));
        contract
            .expect_get_current_block_timestamp()
            .return_const(Ok(U256::from(200)));
        contract
            .expect_withdraw_hash_for_slot()
            .with(eq(slot))
            .return_const(Ok(withdraws.rolling_hash(0)));
        contract
            .expect_get_current_state_root()
            .return_const(Ok(state_hash));
        contract
            .expect_apply_withdraws()
            .with(eq(slot), always(), always(), always(), always())
            .return_const(Ok(()));

        let mut db = MockDbInterface::default();
        db.expect_get_withdraws_of_slot()
            .with(eq(U256::one()))
            .return_const(Ok(withdraws));
        db.expect_get_balances_for_state_root()
            .with(eq(state_hash))
            .return_const(Ok(state));

        assert_eq!(run_withdraw_listener(&db, &contract), Ok(true));
    }

    #[test]
    fn does_not_apply_if_highest_slot_already_applied() {
        let slot = U256::from(1);
        let mut contract = MockSnappContract::default();
        contract
            .expect_get_current_withdraw_slot()
            .return_const(Ok(slot));
        contract
            .expect_has_withdraw_slot_been_applied()
            .with(eq(slot))
            .return_const(Ok(true));

        let db = MockDbInterface::new();
        assert_eq!(run_withdraw_listener(&db, &contract), Ok(false));
    }

    #[test]
    fn does_not_apply_if_highest_slot_too_close_to_current_block() {
        let slot = U256::from(1);
        let mut contract = MockSnappContract::default();
        contract
            .expect_get_current_withdraw_slot()
            .return_const(Ok(slot));
        contract
            .expect_has_withdraw_slot_been_applied()
            .with(eq(slot))
            .return_const(Ok(false));
        contract
            .expect_has_withdraw_slot_been_applied()
            .with(eq(slot - 1))
            .return_const(Ok(true));

        contract
            .expect_creation_timestamp_for_withdraw_slot()
            .with(eq(slot))
            .return_const(Ok(U256::from(10)));
        contract
            .expect_get_current_block_timestamp()
            .return_const(Ok(U256::from(11)));

        let db = MockDbInterface::new();
        assert_eq!(run_withdraw_listener(&db, &contract), Ok(false));
    }

    #[test]
    fn applies_all_unapplied_states_before_current() {
        let slot = U256::from(1);
        let state_hash = H256::zero();
        let first_withdraws = vec![create_flux_for_test(0, 1), create_flux_for_test(0, 2)];
        let second_withdraws = vec![create_flux_for_test(1, 1), create_flux_for_test(1, 2)];

        let mut contract = MockSnappContract::default();
        contract
            .expect_get_current_withdraw_slot()
            .return_const(Ok(slot));

        contract
            .expect_has_withdraw_slot_been_applied()
            .with(eq(slot))
            .return_const(Ok(false));
        contract
            .expect_has_withdraw_slot_been_applied()
            .with(eq(slot - 1))
            .return_const(Ok(false));

        contract
            .expect_creation_timestamp_for_withdraw_slot()
            .with(eq(slot - 1))
            .return_const(Ok(U256::from(10)));

        contract
            .expect_get_current_block_timestamp()
            .return_const(Ok(U256::from(200)));
        contract
            .expect_withdraw_hash_for_slot()
            .with(eq(slot - 1))
            .return_const(Ok(second_withdraws.rolling_hash(0)));

        contract
            .expect_get_current_state_root()
            .return_const(Ok(state_hash));
        contract
            .expect_apply_withdraws()
            .with(eq(slot - 1), always(), always(), always(), always())
            .return_const(Ok(()));

        let state = AccountState::new(
            state_hash,
            U256::one(),
            vec![100; (NUM_TOKENS * 2) as usize],
            NUM_TOKENS,
        );

        let mut db = MockDbInterface::new();
        db.expect_get_withdraws_of_slot()
            .with(eq(U256::zero()))
            .return_const(Ok(first_withdraws));
        db.expect_get_balances_for_state_root()
            .with(eq(state_hash))
            .return_const(Ok(state));

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
            vec![100; (NUM_TOKENS * 2) as usize],
            NUM_TOKENS,
        );

        let mut contract = MockSnappContract::default();
        contract
            .expect_get_current_withdraw_slot()
            .return_const(Ok(slot));
        contract
            .expect_has_withdraw_slot_been_applied()
            .with(eq(slot))
            .return_const(Ok(false));
        contract
            .expect_has_withdraw_slot_been_applied()
            .with(eq(slot - 1))
            .return_const(Ok(true));

        contract
            .expect_creation_timestamp_for_withdraw_slot()
            .with(eq(slot))
            .return_const(Ok(U256::from(10)));
        contract
            .expect_get_current_block_timestamp()
            .return_const(Ok(U256::from(200)));

        contract
            .expect_withdraw_hash_for_slot()
            .with(eq(slot))
            .return_const(Ok(H256::zero()));
        contract
            .expect_get_current_state_root()
            .return_const(Ok(state_hash));

        let mut db = MockDbInterface::new();
        db.expect_get_withdraws_of_slot()
            .with(eq(U256::one()))
            .return_const(Ok(withdraws));
        db.expect_get_balances_for_state_root()
            .with(eq(state_hash))
            .return_const(Ok(state));

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
            vec![100; (NUM_TOKENS * 2) as usize],
            NUM_TOKENS,
        );
        state.decrement_balance(1, H160::from_low_u64_be(0), 100);

        let merkle_root = withdraws.root_hash(&[true, false]);

        let mut contract = MockSnappContract::default();
        contract
            .expect_get_current_withdraw_slot()
            .return_const(Ok(slot));
        contract
            .expect_has_withdraw_slot_been_applied()
            .with(eq(slot))
            .return_const(Ok(false));
        contract
            .expect_has_withdraw_slot_been_applied()
            .with(eq(slot - 1))
            .return_const(Ok(true));
        contract
            .expect_creation_timestamp_for_withdraw_slot()
            .with(eq(slot))
            .return_const(Ok(U256::from(10)));
        contract
            .expect_get_current_block_timestamp()
            .return_const(Ok(U256::from(200)));
        contract
            .expect_withdraw_hash_for_slot()
            .with(eq(slot))
            .return_const(Ok(withdraws.rolling_hash(0)));
        contract
            .expect_get_current_state_root()
            .return_const(Ok(state_hash));
        contract
            .expect_apply_withdraws()
            .with(eq(slot), eq(merkle_root), always(), always(), always())
            .return_const(Ok(()));

        let mut db = MockDbInterface::new();
        db.expect_get_withdraws_of_slot()
            .with(eq(U256::one()))
            .return_const(Ok(withdraws));
        db.expect_get_balances_for_state_root()
            .with(eq(state_hash))
            .return_const(Ok(state));

        assert_eq!(run_withdraw_listener(&db, &contract), Ok(true));
    }
}
