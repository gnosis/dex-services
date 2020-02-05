use log::info;
use std::env;
use std::future::Future;
use web3::types::{H256, U256};

use crate::contracts::snapp_contract::SnappContract;
use crate::error::DriverError;
use crate::error::ErrorKind;
use crate::price_finding::{
    Fee, NaiveSolver, OptimisationPriceFinder, OptimizationModel, PriceFinding,
};

const BATCH_TIME_SECONDS: u32 = 3 * 60;

pub trait CeiledDiv {
    fn ceiled_div(&self, divisor: Self) -> Self;
}

impl CeiledDiv for u128 {
    fn ceiled_div(&self, divisor: u128) -> u128 {
        // ceil(p / float(q)) == (p + q - 1) / q
        (self + divisor - 1) / divisor
    }
}

impl CeiledDiv for U256 {
    fn ceiled_div(&self, divisor: U256) -> U256 {
        //ceil(p / float(q)) == (p + q - 1) / q
        (self + divisor - 1) / divisor
    }
}

pub trait CheckedConvertU128 {
    fn as_u128_checked(&self) -> Option<u128>;
}

impl CheckedConvertU128 for U256 {
    fn as_u128_checked(&self) -> Option<u128> {
        if *self <= U256::from(u128::max_value()) {
            Some(self.low_u128())
        } else {
            None
        }
    }
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

pub fn create_price_finder(fee: Option<Fee>) -> Box<dyn PriceFinding> {
    let optimization_model_string: String =
        env::var("OPTIMIZATION_MODEL").unwrap_or_else(|_| String::from("NAIVE"));
    let optimization_model = OptimizationModel::from(optimization_model_string.clone());

    if optimization_model == OptimizationModel::NAIVE {
        info!("Using naive price finder");
        Box::new(NaiveSolver::new(fee))
    } else {
        info!(
            "Using {:} optimisation price finder",
            optimization_model_string
        );
        Box::new(OptimisationPriceFinder::new(fee, optimization_model))
    }
}

pub trait FutureWaitExt: Future {
    fn wait(self) -> Self::Output;
}

impl<F> FutureWaitExt for F
where
    F: Future,
{
    fn wait(self) -> Self::Output {
        futures::executor::block_on(self)
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;

    #[test]
    fn test_checked_u256_to_u128() {
        assert_eq!(Some(42u128), U256::from(42).as_u128_checked());
        assert_eq!(
            Some(u128::max_value()),
            U256::from(u128::max_value()).as_u128_checked(),
        );
        assert_eq!(
            None,
            (U256::from(u128::max_value()) + U256::one()).as_u128_checked(),
        );
        assert_eq!(None, U256::max_value().as_u128_checked(),);
    }

    #[test]
    fn test_ceiled_div_u128() {
        assert_eq!(0u128.ceiled_div(10), 0);
        assert_eq!(1u128.ceiled_div(10), 1);
        assert_eq!(10u128.ceiled_div(10), 1);
    }

    #[test]
    #[should_panic]
    fn test_ceiled_div_by_0_u128() {
        1u128.ceiled_div(0);
    }

    #[test]
    #[should_panic]
    fn test_ceiled_div_overflow_u128() {
        u128::max_value().ceiled_div(1);
    }

    #[test]
    fn test_ceiled_div_u256() {
        assert_eq!(U256::from(0).ceiled_div(U256::from(10)), U256::from(0));
        assert_eq!(U256::from(1).ceiled_div(U256::from(10)), U256::from(1));
        assert_eq!(U256::from(10).ceiled_div(U256::from(10)), U256::from(1));
    }

    #[test]
    #[should_panic]
    fn test_ceiled_div_by_0_u256() {
        U256::one().ceiled_div(U256::zero());
    }

    #[test]
    #[should_panic]
    fn test_ceiled_div_overflow_u256() {
        U256::max_value().ceiled_div(U256::from(1));
    }
}
