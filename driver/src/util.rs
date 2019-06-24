use crate::contract::SnappContract;
use crate::error::DriverError;
use crate::error::ErrorKind;

use web3::types::{H256, U256};

pub fn find_first_unapplied_slot(
    upper_bound: U256,
    has_slot_been_applied: Box<&dyn Fn(U256) -> Result<bool, DriverError>>,
) -> Result<U256, DriverError> {
    if upper_bound == U256::max_value() {
        return Ok(U256::zero());
    }
    let mut slot = upper_bound + 1;
    while slot != U256::zero() {
        if has_slot_been_applied(slot - 1)? {
            return Ok(slot);
        }
        slot = slot - 1;
    }
    Ok(U256::zero())
}

pub fn hash_consistency_check(
    hash_calculated: H256,
    hash_from_contract: H256,
    flux_type: &str,
) -> Result<(), DriverError> {
    if hash_calculated != hash_from_contract {
        return Err(DriverError::new(
            &format!(
                "Pending {} hash from contract ({:#}), didn't match the one found in db ({:#})",
                flux_type, hash_from_contract, hash_calculated
            ),
            ErrorKind::StateError,
        ));
    }
    Ok(())
}

pub fn can_process<C>(
    slot: U256,
    contract: &C,
    creation_block: Box<&dyn Fn(U256) -> Result<U256, DriverError>>,
) -> Result<bool, DriverError>
where
    C: SnappContract,
{
    let slot_creation_block = creation_block(slot)?;
    if slot_creation_block == U256::zero() {
        return Ok(false);
    }
    let current_block = contract.get_current_block_timestamp()?;
    Ok(slot_creation_block + 180 < current_block)
}
