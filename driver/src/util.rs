use std::env;

use web3::types::{H256, U256};

use crate::contracts::snapp_contract::SnappContract;
use crate::error::DriverError;
use crate::error::ErrorKind;
use crate::price_finding::{LinearOptimisationPriceFinder, NaiveSolver, PriceFinding};

const BATCH_TIME_SECONDS: u32 = 3 * 60;

pub fn u128_to_u256(x: u128) -> U256 {
    U256::from_big_endian(&x.to_be_bytes())
}

pub fn u256_to_u128(x: U256) -> u128 {
    let arr = x.0;
    if arr[2] | arr[3] != 0 {
        panic!("Overflow");
    }
    u128::from(arr[0]) + u128::from(arr[1]) * (2u128).pow(64)
}

pub fn find_first_unapplied_slot(
    upper_bound: U256,
    has_slot_been_applied: &dyn Fn(U256) -> Result<bool, DriverError>,
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

pub enum ProcessingState {
    TooEarly,
    AcceptsBids,
    AcceptsSolution,
}

pub fn batch_processing_state(
    slot: U256,
    contract: &dyn SnappContract,
    creation_block_time: &dyn Fn(U256) -> Result<U256, DriverError>,
) -> Result<ProcessingState, DriverError> {
    let slot_creation_block_time = creation_block_time(slot)?;
    if slot_creation_block_time == U256::zero() {
        return Ok(ProcessingState::TooEarly);
    }

    let current_block_time = contract.get_current_block_timestamp()?;
    if slot_creation_block_time + 2 * BATCH_TIME_SECONDS < current_block_time {
        return Ok(ProcessingState::AcceptsSolution);
    }
    if slot_creation_block_time + BATCH_TIME_SECONDS < current_block_time {
        return Ok(ProcessingState::AcceptsBids);
    }
    Ok(ProcessingState::TooEarly)
}

pub fn create_price_finder() -> Box<dyn PriceFinding> {
    let solver_env_var = env::var("LINEAR_OPTIMIZATION_SOLVER").unwrap_or_else(|_| "0".to_string());
    if solver_env_var == "1" {
        info!("Using linear optimisation price finder");
        Box::new(LinearOptimisationPriceFinder::new())
    } else {
        info!("Using naive price finder");
        Box::new(NaiveSolver::new(None))
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;

    #[test]
    fn test_u128_to_u256_on_one() {
        assert_eq!(U256::from(1), u128_to_u256(1u128));
    }
    #[test]
    fn test_u128_to_u256_on_max() {
        assert_eq!(
            U256::from_dec_str("340282366920938463463374607431768211455").unwrap(),
            u128_to_u256(u128::max_value())
        );
    }

    #[test]
    fn test_256_to_u128_works() {
        assert_eq!(0u128, u256_to_u128(U256::from(0)));
        assert_eq!(1u128, u256_to_u128(U256::from(1)));
        assert_eq!(
            u128::max_value(),
            u256_to_u128(U256::from_dec_str("340282366920938463463374607431768211455").unwrap())
        );
    }
    #[test]
    #[should_panic]
    fn test_u256_to_u128_panics_on_overflow() {
        u256_to_u128(U256::from_dec_str("340282366920938463463374607431768211456").unwrap());
    }
}
