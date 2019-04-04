use crate::contract::SnappContract;
use crate::error::DriverError;

use web3::types::U256;

pub fn find_first_unapplied_slot(
    upper_bound: U256, 
    has_slot_been_applied: Box<&Fn(U256) -> Result<bool, DriverError>>
) -> Result<U256, DriverError>
{
    let mut slot = upper_bound;
    while slot != U256::zero() {
        if has_slot_been_applied(slot - 1)? {
            return Ok(slot)
        }
        slot = slot - 1;
    }
    Ok(U256::zero())
}

pub fn can_process<C>(
    slot: U256, 
    contract: &C,
    creation_block: Box<&Fn(U256) -> Result<U256, DriverError>>,
) -> Result<bool, DriverError> 
    where C: SnappContract
{
    let slot_creation_block = creation_block(slot)?;
    if slot_creation_block == U256::zero() {
        return Ok( false );
    }
    let current_block = contract.get_current_block_number()?;
    Ok(slot_creation_block + 20 < current_block)
}