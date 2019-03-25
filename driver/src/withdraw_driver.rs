use crate::models;
use crate::models::Hashable;

use crate::db_interface::DbInterface;
use crate::contract::SnappContract;
use crate::error::{DriverError, ErrorKind};

use web3::types::{H256, U256};
use sha2::{Digest, Sha256};

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

fn find_first_unapplied_slot<C>(upper_bound: U256, contract: &C) -> Result<U256, DriverError>
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

fn can_process<C>(slot: U256, contract: &C) -> Result<bool, DriverError> 
    where C: SnappContract
{
    let slot_creation_block = contract.creation_block_for_withdraw_slot(slot)?;
    if slot_creation_block == U256::zero() {
        return Ok( false );
    }
    let current_block = contract.get_current_block_number()?;
    Ok(slot_creation_block + 20 < current_block)
}

fn merkleize_valid_withdraws(withdraws: &Vec<models::PendingFlux>, valid: &Vec<bool>) -> H256 {
    assert!(withdraws.len() == valid.len());
    let mut withdraw_bytes = vec![vec![0; 32]; 128];
    for (index, _) in valid.iter().enumerate().filter(|(_, valid)| **valid) {
        withdraw_bytes[index] = withdraws[index].bytes();
    }
    merkleize(withdraw_bytes)
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
    merkleize(next_layer)
}

pub fn run_withdraw_listener<D, C>(db: &D, contract: &C) -> Result<(bool), DriverError> 
    where   D: DbInterface,
            C: SnappContract
{
    let withdraw_slot = contract.get_current_withdraw_slot()?;

    println!("Current top withdraw_slot is {:?}", withdraw_slot);
    let slot = find_first_unapplied_slot(withdraw_slot + 1, contract)?;
    if slot <= withdraw_slot {
        println!("Highest unprocessed withdraw_slot is {:?}", slot);
        if can_process(slot, contract)? {
            println!("Processing withdraw_slot {:?}", slot);
            let state_root = contract.get_current_state_root()?;
            let contract_withdraw_hash = contract.withdraw_hash_for_slot(slot)?;
            let balances = db.get_current_balances(&state_root)?;

            let withdraws = db.get_withdraws_of_slot(slot.low_u32())?;
            let withdraw_hash = withdraws.hash();
            if withdraw_hash != contract_withdraw_hash {
                return Err(DriverError::new(
                    &format!("Pending withdraw hash from contract ({}), didn't match the one found in db ({})", 
                    withdraw_hash, contract_withdraw_hash), ErrorKind::StateError
                ));
            }

            let (updated_balances, valid_withdraws) = apply_withdraws(&balances, &withdraws);
            let withdrawal_merkle_root = merkleize_valid_withdraws(&withdraws, &valid_withdraws);
            let new_state_root = H256::from(updated_balances.hash()?);
            
            println!("New StateHash is {}, Valid Withdraw Merkle Root is {}", new_state_root, withdrawal_merkle_root);
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
    use crate::models::tests::create_flux_for_test;
    use crate::db_interface::tests::DbInterfaceMock;
    use mock_it::Matcher::*;

    #[test]
    fn applies_current_state_if_unapplied_and_enough_blocks_passed() {
        let slot = U256::from(1);
        let state_hash = H256::zero();
        let withdraws = vec![create_flux_for_test(1,1), create_flux_for_test(1,2)];
        let state = models::State {
            stateHash: format!("{:x}", state_hash),
            stateIndex: 1,
            balances: vec![100; (models::TOKENS * 2) as usize],
        };

        let contract = SnappContractMock::new();
        contract.get_current_withdraw_slot.given(()).will_return(Ok(slot));
        contract.has_withdraw_slot_been_applied.given(slot).will_return(Ok(false));
        contract.has_withdraw_slot_been_applied.given(slot - 1).will_return(Ok(true));
        contract.creation_block_for_withdraw_slot.given(slot).will_return(Ok(U256::from(10)));
        contract.get_current_block_number.given(()).will_return(Ok(U256::from(34)));
        contract.withdraw_hash_for_slot.given(slot).will_return(Ok(withdraws.hash()));
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

        contract.creation_block_for_withdraw_slot.given(slot).will_return(Ok(U256::from(10)));
        contract.get_current_block_number.given(()).will_return(Ok(U256::from(11)));

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

        contract.creation_block_for_withdraw_slot.given(slot-1).will_return(Ok(U256::from(10)));

        contract.get_current_block_number.given(()).will_return(Ok(U256::from(34)));
        contract.withdraw_hash_for_slot.given(slot-1).will_return(Ok(second_withdraws.hash()));

        contract.get_current_state_root.given(()).will_return(Ok(state_hash));
        contract.apply_withdraws.given((slot - 1, Any, Any, Any, Any)).will_return(Ok(()));

        let state = models::State {
            stateHash: format!("{:x}", state_hash),
            stateIndex: 1,
            balances: vec![100; (models::TOKENS * 2) as usize],
        };

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

        let state = models::State {
            stateHash: format!("{:x}", state_hash),
            stateIndex: 1,
            balances: vec![100; (models::TOKENS * 2) as usize],
        };

        let contract = SnappContractMock::new();
        contract.get_current_withdraw_slot.given(()).will_return(Ok(slot));
        contract.has_withdraw_slot_been_applied.given(slot).will_return(Ok(false));
        contract.has_withdraw_slot_been_applied.given(slot - 1).will_return(Ok(true));

        contract.creation_block_for_withdraw_slot.given(slot).will_return(Ok(U256::from(10)));
        contract.get_current_block_number.given(()).will_return(Ok(U256::from(34)));
        
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
            slotIndex: 2,
            slot: 1,
            accountId: 1,
            tokenId: 2,
            amount: 10,
        }];
        let mut state = models::State {
            stateHash: format!("{:x}", state_hash),
            stateIndex: 1,
            balances: vec![100; (models::TOKENS * 2) as usize],
        };

        state.balances[1] = 0;

        let merkle_root = merkleize_valid_withdraws(&withdraws, &vec![true, false]);

        let contract = SnappContractMock::new();
        contract.get_current_withdraw_slot.given(()).will_return(Ok(slot));
        contract.has_withdraw_slot_been_applied.given(slot).will_return(Ok(false));
        contract.has_withdraw_slot_been_applied.given(slot - 1).will_return(Ok(true));
        contract.creation_block_for_withdraw_slot.given(slot).will_return(Ok(U256::from(10)));
        contract.get_current_block_number.given(()).will_return(Ok(U256::from(34)));
        contract.withdraw_hash_for_slot.given(slot).will_return(Ok(withdraws.hash()));
        contract.get_current_state_root.given(()).will_return(Ok(state_hash));
        contract.apply_withdraws.given((slot, Val(merkle_root), Any, Any, Any)).will_return(Ok(()));

        let db = DbInterfaceMock::new();
        db.get_withdraws_of_slot.given(1).will_return(Ok(withdraws));
        db.get_current_balances.given(state_hash).will_return(Ok(state));

        assert_eq!(run_withdraw_listener(&db, &contract), Ok(true));
    }
}