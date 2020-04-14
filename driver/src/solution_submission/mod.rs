#![allow(clippy::ptr_arg)] // required for automock

use crate::contracts::stablex_contract::StableXContract;
use crate::models::Solution;

use crate::gas_station::GasPriceEstimating;
use anyhow::{Error, Result};
use ethcontract::errors::{ExecutionError, MethodError};
use ethcontract::web3::types::TransactionReceipt;
use ethcontract::U256;
use log::info;
#[cfg(test)]
use mockall::automock;
use std::thread;
use std::time::Duration;
use thiserror::Error;

/// The amount of time the solution submitter should wait between polling the
/// current batch ID to wait for a block to be mined after the solving batch
/// stops accepting orders.
#[cfg(not(test))]
const POLL_TIMEOUT: Duration = Duration::from_secs(5);
#[cfg(test)]
const POLL_TIMEOUT: Duration = Duration::from_secs(0);

#[cfg_attr(test, automock)]
pub trait StableXSolutionSubmitting {
    /// Return the objective value for the given solution in the given
    /// batch or an error.
    ///
    /// # Arguments
    /// * `batch_index` - the auction for which this solutions should be evaluated
    /// * `orders` - the list of orders for which this solution is applicable
    /// * `solution` - the solution to be evaluated
    fn get_solution_objective_value(
        &self,
        batch_index: U256,
        solution: Solution,
    ) -> Result<U256, SolutionSubmissionError>;

    /// Submits the provided solution and returns the result of the submission
    ///
    /// # Arguments
    /// * `batch_index` - the auction for which this solutions should be evaluated
    /// * `orders` - the list of orders for which this solution is applicable
    /// * `solution` - the solution to be evaluated
    /// * `claimed_objective_value` - the objective value of the provided solution.
    fn submit_solution(
        &self,
        batch_index: U256,
        solution: Solution,
        claimed_objective_value: U256,
    ) -> Result<(), SolutionSubmissionError>;
}

/// An error with verifying or submitting a solution
#[derive(Debug, Error)]
pub enum SolutionSubmissionError {
    #[error("Benign Error: {0}")]
    Benign(String),
    #[error("Unexpected Error: {0}")]
    Unexpected(Error),
}

impl From<Error> for SolutionSubmissionError {
    fn from(err: Error) -> Self {
        err.downcast_ref::<MethodError>()
            .and_then(|method_error| match &method_error.inner {
                ExecutionError::Revert(Some(reason)) => {
                    let reason_slice: &str = &*reason;
                    match reason_slice {
                        "New objective doesn\'t sufficiently improve current solution" => {
                            Some(SolutionSubmissionError::Benign(reason.clone()))
                        }
                        "Claimed objective doesn't sufficiently improve current solution" => {
                            Some(SolutionSubmissionError::Benign(reason.clone()))
                        }
                        "SafeMath: subtraction overflow" => {
                            Some(SolutionSubmissionError::Benign(reason.clone()))
                        }
                        _ => None,
                    }
                }
                _ => None,
            })
            .unwrap_or_else(|| SolutionSubmissionError::Unexpected(err))
    }
}

pub struct StableXSolutionSubmitter<'a> {
    contract: &'a (dyn StableXContract + Sync),
    gas_price_estimating: &'a (dyn GasPriceEstimating + Sync),
}

impl<'a> StableXSolutionSubmitter<'a> {
    pub fn new(
        contract: &'a (dyn StableXContract + Sync),
        gas_price_estimating: &'a (dyn GasPriceEstimating + Sync),
    ) -> Self {
        Self {
            contract,
            gas_price_estimating,
        }
    }
}

impl<'a> StableXSolutionSubmitting for StableXSolutionSubmitter<'a> {
    fn get_solution_objective_value(
        &self,
        batch_index: U256,
        solution: Solution,
    ) -> Result<U256, SolutionSubmissionError> {
        // NOTE: Compare with `>=` as the exchange's current batch index is the
        //   one accepting orders and does not yet accept solutions.
        while batch_index.as_u32() >= self.contract.get_current_auction_index()? {
            info!("Solved batch is not yet accepting solutions, waiting for next batch.");
            thread::sleep(POLL_TIMEOUT);
        }

        self.contract
            .get_solution_objective_value(batch_index, solution, None)
            .map_err(SolutionSubmissionError::from)
    }

    fn submit_solution(
        &self,
        batch_index: U256,
        solution: Solution,
        claimed_objective_value: U256,
    ) -> Result<(), SolutionSubmissionError> {
        retry_with_gas_price_increase(
            self.contract,
            batch_index,
            solution.clone(),
            claimed_objective_value,
            self.gas_price_estimating,
            60_000_000_000u64.into(),
        )
        .map_err(|err| {
            extract_transaction_receipt(&err)
                .and_then(|tx| {
                    let block_number = tx.block_number?;
                    match self.contract.get_solution_objective_value(
                        batch_index,
                        solution,
                        Some(block_number.into()),
                    ) {
                        Ok(_) => None,
                        Err(e) => Some(SolutionSubmissionError::from(e)),
                    }
                })
                .unwrap_or_else(|| SolutionSubmissionError::Unexpected(err.into()))
        })
        .map(|_| ())
    }
}

fn retry_with_gas_price_increase(
    contract: &dyn StableXContract,
    batch_index: U256,
    solution: Solution,
    claimed_objective_value: U256,
    gas_price_estimating: &dyn GasPriceEstimating,
    gas_cap: U256,
) -> Result<(), MethodError> {
    const INCREASE_FACTOR: u32 = 2;
    const BLOCK_TIMEOUT: usize = 1;
    const DEFAULT_GAS_PRICE: u64 = 15_000_000_000;

    let mut gas_price = match gas_price_estimating.estimate_gas_price() {
        Ok(gas_estimate) => gas_estimate.fast,
        Err(ref err) => {
            log::warn!(
                "failed to get gas price from gnosis safe gas station: {}",
                err
            );
            U256::from(DEFAULT_GAS_PRICE)
        }
    };
    // Never exceed the gas cap.
    gas_price = std::cmp::min(gas_price, gas_cap);

    let mut result;
    // the following block emulates a do-while loop
    while {
        // Submit solution at estimated gas price (not exceeding the cap)
        result = contract.submit_solution(
            batch_index,
            solution.clone(),
            claimed_objective_value,
            gas_price,
            if gas_price == gas_cap {
                None
            } else {
                Some(BLOCK_TIMEOUT)
            },
        );
        // Breaking condition for our loop
        gas_price < gas_cap
            && matches!(
                result,
                Err(MethodError {
                    inner: ExecutionError::ConfirmTimeout,
                    ..
                })
            )
    } {
        // Increase gas
        gas_price = std::cmp::min(gas_price * INCREASE_FACTOR, gas_cap);
    }

    result
}

fn extract_transaction_receipt(err: &MethodError) -> Option<&TransactionReceipt> {
    match &err.inner {
        ExecutionError::Failure(tx) => Some(tx.as_ref()),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::contracts::stablex_contract::MockStableXContract;
    use crate::gas_station::{GasPrice, MockGasPriceEstimating};

    use anyhow::anyhow;
    use ethcontract::web3::types::H2048;
    use ethcontract::H256;
    use mockall::predicate::{always, eq};

    #[test]
    fn solution_submitter_waits_for_solving_batch() {
        let mut contract = MockStableXContract::new();

        contract
            .expect_get_current_auction_index()
            .times(2)
            .returning(|| Ok(0));
        contract
            .expect_get_current_auction_index()
            .times(1)
            .returning(|| Ok(1));

        contract
            .expect_get_solution_objective_value()
            .return_once(move |_, _, _| Ok(U256::from(42)));

        let gas_station = MockGasPriceEstimating::new();

        let submitter = StableXSolutionSubmitter::new(&contract, &gas_station);
        let result = submitter.get_solution_objective_value(U256::zero(), Solution::trivial());

        contract.checkpoint();
        assert_eq!(result.unwrap(), U256::from(42));
    }

    #[test]
    fn test_retry_with_gas_price_increase_once() {
        let mut contract = MockStableXContract::new();
        contract
            .expect_submit_solution()
            .times(1)
            .with(always(), always(), always(), eq(U256::from(5)), eq(Some(1)))
            .return_once(|_, _, _, _, _| {
                Err(MethodError::from_parts(
                    "submitSolution(uint32,uint256,address[],uint16[],uint128[],uint128[],uint16[])"
                        .to_owned(),
                    ExecutionError::ConfirmTimeout,
                ))
            });
        contract
            .expect_submit_solution()
            .with(always(), always(), always(), eq(U256::from(9)), eq(None))
            .return_once(|_, _, _, _, _| Ok(()));

        let mut gas_station = MockGasPriceEstimating::new();
        gas_station.expect_estimate_gas_price().return_once(|| {
            Ok(GasPrice {
                fast: 5.into(),
                ..Default::default()
            })
        });

        retry_with_gas_price_increase(
            &contract,
            1.into(),
            Solution::trivial(),
            1.into(),
            &gas_station,
            9.into(),
        )
        .unwrap();
    }

    #[test]
    fn test_retry_with_gas_price_increase_timeout() {
        let mut contract = MockStableXContract::new();
        contract
            .expect_submit_solution()
            .times(1)
            .with(always(), always(), always(), eq(U256::from(5)), eq(Some(1)))
            .return_once(|_, _, _, _, _| {
                Err(MethodError::from_parts(
                    "submitSolution(uint32,uint256,address[],uint16[],uint128[],uint128[],uint16[])"
                        .to_owned(),
                    ExecutionError::ConfirmTimeout,
                ))
            });
        contract
            .expect_submit_solution()
            .with(always(), always(), always(), eq(U256::from(9)), eq(None))
            .return_once(|_, _, _, _, _| {
                Err(MethodError::from_parts(
                    "submitSolution(uint32,uint256,address[],uint16[],uint128[],uint128[],uint16[])"
                        .to_owned(),
                    ExecutionError::ConfirmTimeout,
                ))
            });

        let mut gas_station = MockGasPriceEstimating::new();
        gas_station.expect_estimate_gas_price().return_once(|| {
            Ok(GasPrice {
                fast: 5.into(),
                ..Default::default()
            })
        });
        assert!(retry_with_gas_price_increase(
            &contract,
            1.into(),
            Solution::trivial(),
            1.into(),
            &gas_station,
            9.into(),
        )
        .is_err())
    }

    #[test]
    fn test_benign_verification_failure() {
        let mut contract = MockStableXContract::new();

        contract
            .expect_get_current_auction_index()
            .return_once(|| Ok(1));
        contract
            .expect_get_solution_objective_value()
            .return_once(move |_, _, _| {
                Err(anyhow!(MethodError::from_parts(
                    "submitSolution(uint32,uint256,address[],uint16[],uint128[],uint128[],uint16[])"
                        .to_owned(),
                    ExecutionError::Revert(Some("SafeMath: subtraction overflow".to_owned())),
                )))
            });
        let gas_station = MockGasPriceEstimating::new();

        let submitter = StableXSolutionSubmitter::new(&contract, &gas_station);
        let result = submitter.get_solution_objective_value(U256::zero(), Solution::trivial());

        match result.expect_err("Should have errored") {
            SolutionSubmissionError::Benign(_) => (),
            SolutionSubmissionError::Unexpected(err) => {
                panic!("Expecting benign failure, but got {}", err)
            }
        };
    }

    #[test]
    fn test_benign_solution_submission_failure() {
        let mut contract = MockStableXContract::new();

        let tx_hash = H256::zero();
        let block_number = 42.into();
        let receipt = TransactionReceipt {
            transaction_hash: tx_hash,
            transaction_index: 0.into(),
            block_hash: None,
            block_number: Some(block_number),
            cumulative_gas_used: U256::zero(),
            gas_used: None,
            contract_address: None,
            logs: vec![],
            status: None,
            logs_bloom: H2048::zero(),
        };

        // Submit Solution returns failed tx
        contract
            .expect_submit_solution()
            .return_once(move |_, _, _, _, _| {
                Err(MethodError::from_parts(
                    "submitSolution(uint32,uint256,address[],uint16[],uint128[],uint128[],uint16[])"
                        .to_owned(),
                    ExecutionError::Failure(Box::new(receipt)),
                ))
            });
        // Get objective value on old block number returns revert reason
        contract
            .expect_get_solution_objective_value()
            .with(always(), always(), eq(Some(block_number.into())))
            .return_once(move |_, _, _| {
                Err(anyhow!(MethodError::from_parts(
                    "submitSolution(uint32,uint256,address[],uint16[],uint128[],uint128[],uint16[])"
                        .to_owned(),
                    ExecutionError::Revert(Some("Claimed objective doesn't sufficiently improve current solution".to_owned())),
                )))
            });
        let mut gas_station = MockGasPriceEstimating::new();
        gas_station.expect_estimate_gas_price().return_once(|| {
            Ok(GasPrice {
                fast: 5.into(),
                ..Default::default()
            })
        });

        let submitter = StableXSolutionSubmitter::new(&contract, &gas_station);
        let result = submitter.submit_solution(U256::zero(), Solution::trivial(), U256::zero());

        match result.expect_err("Should have errored") {
            SolutionSubmissionError::Benign(_) => (),
            SolutionSubmissionError::Unexpected(err) => {
                panic!("Expecting benign failure, but got {}", err)
            }
        };
    }
}
