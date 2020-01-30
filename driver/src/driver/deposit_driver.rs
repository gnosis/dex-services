use crate::contracts::snapp_contract::SnappContract;
use crate::error::DriverError;
use crate::util::{
    batch_processing_state, find_first_unapplied_slot, hash_consistency_check, ProcessingState,
};

use dfusion_core::database::DbInterface;
use dfusion_core::models::RollingHashable;
use log::{debug, info};

pub fn run_deposit_listener(
    db: &dyn DbInterface,
    contract: &dyn SnappContract,
) -> Result<bool, DriverError> {
    let deposit_slot = contract.get_current_deposit_slot()?;

    debug!("Current top deposit_slot is {:?}", deposit_slot);
    let slot =
        find_first_unapplied_slot(deposit_slot, &|i| contract.has_deposit_slot_been_applied(i))?;
    if slot <= deposit_slot {
        debug!("Highest unprocessed deposit_slot is {:?}", slot);
        let processing_state = batch_processing_state(slot, contract, &|i| {
            contract.creation_timestamp_for_deposit_slot(i)
        })?;
        match processing_state {
            ProcessingState::TooEarly => debug!("All deposits are already processed"),
            ProcessingState::AcceptsBids | ProcessingState::AcceptsSolution => {
                info!("Processing deposit_slot {:?}", slot);
                let state_root = contract.get_current_state_root()?;
                let contract_deposit_hash = contract.deposit_hash_for_slot(slot)?;
                let mut balances = db.get_balances_for_state_root(&state_root)?;

                let deposits = db.get_deposits_of_slot(&slot)?;
                let deposit_hash = deposits.rolling_hash(0);
                hash_consistency_check(deposit_hash, contract_deposit_hash, "deposit")?;

                balances.apply_deposits(&deposits);
                info!("New AccountState hash is {}", balances.state_hash);
                contract.apply_deposits(
                    slot,
                    state_root,
                    balances.state_hash,
                    contract_deposit_hash,
                )?;
                return Ok(true);
            }
        }
    } else {
        debug!("No pending deposit batches.");
    }
    Ok(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::contracts::snapp_contract::MockSnappContract;
    use crate::error::ErrorKind;
    use dfusion_core::database::MockDbInterface;
    use dfusion_core::models;
    use dfusion_core::models::flux::tests::create_flux_for_test;
    use mockall::predicate::*;
    use web3::types::{H256, U256};

    const NUM_TOKENS: u16 = 30;

    #[test]
    fn applies_current_state_if_unapplied_and_enough_blocks_passed() {
        let slot = U256::from(1);
        let state_hash = H256::zero();
        let deposits = vec![create_flux_for_test(1, 1), create_flux_for_test(1, 2)];
        let state = models::AccountState::new(
            state_hash,
            U256::one(),
            vec![100; (NUM_TOKENS * 2) as usize],
            NUM_TOKENS,
        );

        let mut contract = MockSnappContract::default();
        contract
            .expect_get_current_deposit_slot()
            .return_const(Ok(slot));
        contract
            .expect_has_deposit_slot_been_applied()
            .with(eq(slot))
            .return_const(Ok(false));
        contract
            .expect_has_deposit_slot_been_applied()
            .with(eq(slot - 1))
            .return_const(Ok(true));
        contract
            .expect_creation_timestamp_for_deposit_slot()
            .with(eq(slot))
            .return_const(Ok(U256::from(10)));
        contract
            .expect_get_current_block_timestamp()
            .return_const(Ok(U256::from(200)));
        contract
            .expect_deposit_hash_for_slot()
            .with(eq(slot))
            .return_const(Ok(deposits.rolling_hash(0)));
        contract
            .expect_get_current_state_root()
            .return_const(Ok(state_hash));
        contract
            .expect_apply_deposits()
            .with(eq(slot), always(), always(), always())
            .return_const(Ok(()));

        let mut db = MockDbInterface::new();
        db.expect_get_deposits_of_slot()
            .with(eq(U256::one()))
            .return_const(Ok(deposits));
        db.expect_get_balances_for_state_root()
            .with(eq(state_hash))
            .return_const(Ok(state));

        assert_eq!(run_deposit_listener(&db, &contract), Ok(true));
    }

    #[test]
    fn does_not_apply_if_highest_slot_already_applied() {
        let slot = U256::from(1);
        let state_hash = H256::zero();

        let state = models::AccountState::new(
            state_hash,
            U256::one(),
            vec![100; (NUM_TOKENS * 2) as usize],
            NUM_TOKENS,
        );

        let mut contract = MockSnappContract::default();
        contract
            .expect_get_current_state_root()
            .return_const(Ok(state_hash));
        contract
            .expect_get_current_deposit_slot()
            .return_const(Ok(slot));
        contract
            .expect_has_deposit_slot_been_applied()
            .with(eq(slot))
            .return_const(Ok(true));

        contract
            .expect_get_current_block_timestamp()
            .return_const(Ok(U256::from(11)));
        contract
            .expect_creation_timestamp_for_deposit_slot()
            .with(eq(slot + 1))
            .return_const(Ok(U256::from(10)));
        contract
            .expect_deposit_hash_for_slot()
            .with(eq(slot + 1))
            .return_const(Ok(H256::zero()));

        let mut db = MockDbInterface::new();
        db.expect_get_balances_for_state_root()
            .with(eq(state_hash))
            .return_const(Ok(state));

        assert_eq!(run_deposit_listener(&db, &contract), Ok(false));
    }

    #[test]
    fn does_not_apply_if_highest_slot_too_close_to_current_block() {
        let slot = U256::from(1);
        let state_hash = H256::zero();
        let deposits = vec![create_flux_for_test(1, 1), create_flux_for_test(1, 2)];

        let state = models::AccountState::new(
            state_hash,
            U256::one(),
            vec![100; (NUM_TOKENS * 2) as usize],
            NUM_TOKENS,
        );

        let mut contract = MockSnappContract::default();
        contract
            .expect_get_current_state_root()
            .return_const(Ok(state_hash));
        contract
            .expect_get_current_deposit_slot()
            .return_const(Ok(slot));
        contract
            .expect_has_deposit_slot_been_applied()
            .with(eq(slot))
            .return_const(Ok(false));
        contract
            .expect_has_deposit_slot_been_applied()
            .with(eq(slot - 1))
            .return_const(Ok(true));

        contract
            .expect_creation_timestamp_for_deposit_slot()
            .with(eq(slot))
            .return_const(Ok(U256::from(10)));
        contract
            .expect_get_current_block_timestamp()
            .return_const(Ok(U256::from(11)));
        contract
            .expect_deposit_hash_for_slot()
            .with(eq(slot))
            .return_const(Ok(deposits.rolling_hash(0)));

        let mut db = MockDbInterface::new();
        db.expect_get_balances_for_state_root()
            .with(eq(state_hash))
            .return_const(Ok(state));

        assert_eq!(run_deposit_listener(&db, &contract), Ok(false));
    }

    #[test]
    fn applies_all_unapplied_states_before_current() {
        let slot = U256::from(1);
        let state_hash = H256::zero();
        let first_deposits = vec![create_flux_for_test(0, 1), create_flux_for_test(0, 2)];
        let second_deposits = vec![create_flux_for_test(1, 1), create_flux_for_test(1, 2)];

        let mut contract = MockSnappContract::default();
        contract
            .expect_get_current_deposit_slot()
            .return_const(Ok(slot));

        contract
            .expect_has_deposit_slot_been_applied()
            .with(eq(slot))
            .return_const(Ok(false));
        contract
            .expect_has_deposit_slot_been_applied()
            .with(eq(slot - 1))
            .return_const(Ok(false));

        contract
            .expect_creation_timestamp_for_deposit_slot()
            .with(eq(slot - 1))
            .return_const(Ok(U256::from(10)));
        contract
            .expect_get_current_block_timestamp()
            .return_const(Ok(U256::from(200)));
        contract
            .expect_deposit_hash_for_slot()
            .with(eq(slot - 1))
            .return_const(Ok(second_deposits.rolling_hash(0)));

        contract
            .expect_get_current_state_root()
            .return_const(Ok(state_hash));
        contract
            .expect_apply_deposits()
            .with(eq(slot - 1), always(), always(), always())
            .return_const(Ok(()));

        let state = models::AccountState::new(
            state_hash,
            U256::one(),
            vec![100; (NUM_TOKENS * 2) as usize],
            NUM_TOKENS,
        );

        let mut db = MockDbInterface::new();
        db.expect_get_deposits_of_slot()
            .with(eq(U256::zero()))
            .return_const(Ok(first_deposits));
        db.expect_get_balances_for_state_root()
            .with(eq(state_hash))
            .return_const(Ok(state));

        assert_eq!(run_deposit_listener(&db, &contract), Ok(true));
    }

    #[test]
    fn returns_error_if_db_deposit_hash_doesnt_match_contract() {
        let slot = U256::from(1);
        let state_hash = H256::zero();

        let deposits = vec![create_flux_for_test(1, 1), create_flux_for_test(1, 2)];

        let state = models::AccountState::new(
            state_hash,
            U256::one(),
            vec![100; (NUM_TOKENS * 2) as usize],
            NUM_TOKENS,
        );

        let mut contract = MockSnappContract::default();
        contract
            .expect_get_current_deposit_slot()
            .return_const(Ok(slot));
        contract
            .expect_has_deposit_slot_been_applied()
            .with(eq(slot))
            .return_const(Ok(false));
        contract
            .expect_has_deposit_slot_been_applied()
            .with(eq(slot - 1))
            .return_const(Ok(true));

        contract
            .expect_creation_timestamp_for_deposit_slot()
            .with(eq(slot))
            .return_const(Ok(U256::from(10)));
        contract
            .expect_get_current_block_timestamp()
            .return_const(Ok(U256::from(200)));

        contract
            .expect_deposit_hash_for_slot()
            .with(eq(slot))
            .return_const(Ok(H256::zero()));
        contract
            .expect_get_current_state_root()
            .return_const(Ok(state_hash));

        let mut db = MockDbInterface::new();
        db.expect_get_deposits_of_slot()
            .with(eq(U256::one()))
            .return_const(Ok(deposits));
        db.expect_get_balances_for_state_root()
            .with(eq(state_hash))
            .return_const(Ok(state));

        let error = run_deposit_listener(&db, &contract).expect_err("Expected Error");
        assert_eq!(error.kind, ErrorKind::StateError);
    }
}
