#![allow(clippy::ptr_arg)] // required for automock

use crate::contracts::stablex_contract::StableXContract;
use crate::gas_station::GasPriceEstimating;
use crate::models::Solution;

use anyhow::{Error, Result};
use ethcontract::errors::{ExecutionError, MethodError};
use ethcontract::web3::types::TransactionReceipt;
use ethcontract::U256;
use futures::future::{BoxFuture, FutureExt as _};
use log::info;
#[cfg(test)]
use mockall::automock;
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
    fn get_solution_objective_value<'a>(
        &'a self,
        batch_index: u32,
        solution: Solution,
    ) -> BoxFuture<'a, Result<U256, SolutionSubmissionError>>;

    /// Submits the provided solution and returns the result of the submission
    ///
    /// # Arguments
    /// * `batch_index` - the auction for which this solutions should be evaluated
    /// * `orders` - the list of orders for which this solution is applicable
    /// * `solution` - the solution to be evaluated
    /// * `claimed_objective_value` - the objective value of the provided solution.
    fn submit_solution<'a>(
        &'a self,
        batch_index: u32,
        solution: Solution,
        claimed_objective_value: U256,
    ) -> BoxFuture<'a, Result<(), SolutionSubmissionError>>;
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

    /// Turn a method error from a solution submission into a SolutionSubmissionError.
    async fn make_error(
        &self,
        batch_index: u32,
        solution: Solution,
        err: MethodError,
    ) -> SolutionSubmissionError {
        if let Some(tx) = extract_transaction_receipt(&err) {
            if let Some(block_number) = tx.block_number {
                if let Err(err) = self
                    .contract
                    .get_solution_objective_value(batch_index, solution, Some(block_number.into()))
                    .await
                {
                    return SolutionSubmissionError::from(err);
                }
            }
        }
        SolutionSubmissionError::Unexpected(err.into())
    }
}

impl<'a> StableXSolutionSubmitting for StableXSolutionSubmitter<'a> {
    fn get_solution_objective_value(
        &self,
        batch_index: u32,
        solution: Solution,
    ) -> BoxFuture<Result<U256, SolutionSubmissionError>> {
        async move {
            // NOTE: Compare with `>=` as the exchange's current batch index is the
            //   one accepting orders and does not yet accept solutions.
            while batch_index >= self.contract.get_current_auction_index().await? {
                info!("Solved batch is not yet accepting solutions, waiting for next batch.");
                if POLL_TIMEOUT > Duration::from_secs(0) {
                    futures_timer::Delay::new(POLL_TIMEOUT).await;
                }
            }

            self.contract
                .get_solution_objective_value(batch_index, solution, None)
                .await
                .map_err(SolutionSubmissionError::from)
        }
        .boxed()
    }

    fn submit_solution(
        &self,
        batch_index: u32,
        solution: Solution,
        claimed_objective_value: U256,
    ) -> BoxFuture<Result<(), SolutionSubmissionError>> {
        async move {
            match retry_with_gas_price_increase(
                self.contract,
                batch_index,
                solution.clone(),
                claimed_objective_value,
                self.gas_price_estimating,
                60_000_000_000u64.into(),
            )
            .await
            {
                Ok(()) => Ok(()),
                Err(err) => Err(self.make_error(batch_index, solution, err).await),
            }
        }
        .boxed()
    }
}

async fn retry_with_gas_price_increase(
    contract: &(dyn StableXContract + Sync),
    batch_index: u32,
    solution: Solution,
    claimed_objective_value: U256,
    gas_price_estimating: &(dyn GasPriceEstimating + Sync),
    gas_cap: U256,
) -> Result<(), MethodError> {
    const INCREASE_FACTOR: u32 = 2;
    const BLOCK_TIMEOUT: usize = 2;
    const DEFAULT_GAS_PRICE: u64 = 15_000_000_000;
    // openethereum requires that the gas price of the resubmitted transaction has increased by at
    // least 12.5%. Rounded to 13% here in case of floating point error or in case the bound becomes
    // exclusive.
    const MIN_GAS_PRICE_INCREASE_FACTOR: f64 = 1.13;

    let mut gas_price_estimate = U256::from(DEFAULT_GAS_PRICE);
    let mut gas_price_factor = 1;
    let mut result;
    // the following block emulates a do-while loop
    while {
        gas_price_estimate = match gas_price_estimating.estimate_gas_price().await {
            Ok(gas_estimate) => gas_estimate.fast,
            Err(ref err) => {
                log::warn!(
                    "failed to get gas price from gnosis safe gas station: {}",
                    err
                );
                gas_price_estimate
            }
        };
        // Never exceed the gas cap.
        let gas_price = std::cmp::min(gas_price_estimate * gas_price_factor, gas_cap);

        // Submit solution at estimated gas price (not exceeding the cap)
        result = contract
            .submit_solution(
                batch_index,
                solution.clone(),
                claimed_objective_value,
                gas_price,
                if gas_price == gas_cap {
                    None
                } else {
                    Some(BLOCK_TIMEOUT)
                },
            )
            .await;

        let gas_price_can_still_increase =
            gas_price.as_u128() as f64 * MIN_GAS_PRICE_INCREASE_FACTOR < gas_cap.as_u128() as f64;
        // Breaking condition for our loop
        gas_price_can_still_increase
            && matches!(
                result,
                Err(MethodError {
                    inner: ExecutionError::ConfirmTimeout,
                    ..
                })
            )
    } {
        // Increase gas
        gas_price_factor *= INCREASE_FACTOR;
        info!(
            "retrying solution submission with increased gas price factor of {}",
            gas_price_factor,
        );
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
            .returning(|| async { Ok(0) }.boxed());
        contract
            .expect_get_current_auction_index()
            .times(1)
            .returning(|| async { Ok(1) }.boxed());

        contract
            .expect_get_solution_objective_value()
            .return_once(move |_, _, _| async { Ok(U256::from(42)) }.boxed());

        let gas_station = MockGasPriceEstimating::new();

        let submitter = StableXSolutionSubmitter::new(&contract, &gas_station);
        let result = submitter
            .get_solution_objective_value(0, Solution::trivial())
            .now_or_never()
            .unwrap();
        contract.checkpoint();
        assert_eq!(result.unwrap(), U256::from(42));
    }

    #[test]
    fn test_retry_with_gas_price_increase_once() {
        let mut contract = MockStableXContract::new();
        contract
            .expect_submit_solution()
            .times(1)
            .with(always(), always(), always(), eq(U256::from(5)), eq(Some(2)))
            .return_once(|_, _, _, _, _| {
                async {
                    Err(MethodError::from_parts(
                    "submitSolution(uint32,uint256,address[],uint16[],uint128[],uint128[],uint16[])"
                        .to_owned(),
                    ExecutionError::ConfirmTimeout,
                ))
                }
                .boxed()
            });
        contract
            .expect_submit_solution()
            .with(always(), always(), always(), eq(U256::from(9)), eq(None))
            .return_once(|_, _, _, _, _| async { Ok(()) }.boxed());

        let mut gas_station = MockGasPriceEstimating::new();
        gas_station.expect_estimate_gas_price().returning(|| {
            async {
                Ok(GasPrice {
                    fast: 5.into(),
                    ..Default::default()
                })
            }
            .boxed()
        });

        retry_with_gas_price_increase(
            &contract,
            1,
            Solution::trivial(),
            1.into(),
            &gas_station,
            9.into(),
        )
        .now_or_never()
        .unwrap()
        .unwrap();
    }

    #[test]
    fn test_retry_with_gas_price_respects_minimum_increase() {
        let mut contract = MockStableXContract::new();
        contract
            .expect_submit_solution()
            .times(1)
            .with(always(), always(), always(), eq(U256::from(90)), always())
            .return_once(|_, _, _, _, _| {
                async {
                    Err(MethodError::from_parts(
                    "submitSolution(uint32,uint256,address[],uint16[],uint128[],uint128[],uint16[])"
                        .to_owned(),
                    ExecutionError::ConfirmTimeout,
                ))
                }
                .boxed()
            });
        // There should not be a second call to submit_solution because 90 to 100 is not a large
        // enough gas price increase.

        let mut gas_station = MockGasPriceEstimating::new();
        gas_station.expect_estimate_gas_price().returning(|| {
            async {
                Ok(GasPrice {
                    fast: 90.into(),
                    ..Default::default()
                })
            }
            .boxed()
        });

        assert!(retry_with_gas_price_increase(
            &contract,
            1,
            Solution::trivial(),
            1.into(),
            &gas_station,
            100.into(),
        )
        .now_or_never()
        .unwrap()
        .is_err());
    }

    #[test]
    fn test_retry_with_gas_price_increase_timeout() {
        let mut contract = MockStableXContract::new();
        contract
            .expect_submit_solution()
            .times(1)
            .with(always(), always(), always(), eq(U256::from(5)), eq(Some(2)))
            .return_once(|_, _, _, _, _| {
                async {
                    Err(MethodError::from_parts(
                    "submitSolution(uint32,uint256,address[],uint16[],uint128[],uint128[],uint16[])"
                        .to_owned(),
                    ExecutionError::ConfirmTimeout,
                ))
                }
                .boxed()
            });
        contract
            .expect_submit_solution()
            .times(1)
            .with(
                always(),
                always(),
                always(),
                eq(U256::from(12)),
                eq(Some(2)),
            )
            .return_once(|_, _, _, _, _| {
                async {
                    Err(MethodError::from_parts(
                    "submitSolution(uint32,uint256,address[],uint16[],uint128[],uint128[],uint16[])"
                        .to_owned(),
                    ExecutionError::ConfirmTimeout,
                ))
                }
                .boxed()
            });
        contract
            .expect_submit_solution()
            .with(always(), always(), always(), eq(U256::from(15)), eq(None))
            .return_once(|_, _, _, _, _| {
                async {
                    Err(MethodError::from_parts(
                    "submitSolution(uint32,uint256,address[],uint16[],uint128[],uint128[],uint16[])"
                        .to_owned(),
                    ExecutionError::ConfirmTimeout,
                ))
                }
                .boxed()
            });

        let mut gas_station = MockGasPriceEstimating::new();
        gas_station
            .expect_estimate_gas_price()
            .times(1)
            .return_once(|| {
                async {
                    Ok(GasPrice {
                        fast: 5.into(),
                        ..Default::default()
                    })
                }
                .boxed()
            });
        gas_station.expect_estimate_gas_price().returning(|| {
            async {
                Ok(GasPrice {
                    fast: 6.into(),
                    ..Default::default()
                })
            }
            .boxed()
        });

        assert!(retry_with_gas_price_increase(
            &contract,
            1,
            Solution::trivial(),
            1.into(),
            &gas_station,
            15.into(),
        )
        .now_or_never()
        .unwrap()
        .is_err())
    }

    #[test]
    fn test_benign_verification_failure() {
        let mut contract = MockStableXContract::new();

        contract
            .expect_get_current_auction_index()
            .return_once(|| async { Ok(1) }.boxed());
        contract
            .expect_get_solution_objective_value()
            .return_once(move |_, _, _| {
                async {
                    Err(anyhow!(MethodError::from_parts(
                    "submitSolution(uint32,uint256,address[],uint16[],uint128[],uint128[],uint16[])"
                        .to_owned(),
                    ExecutionError::Revert(Some("SafeMath: subtraction overflow".to_owned())),
                )))
                }
                .boxed()
            });
        let gas_station = MockGasPriceEstimating::new();

        let submitter = StableXSolutionSubmitter::new(&contract, &gas_station);
        let result = submitter
            .get_solution_objective_value(0, Solution::trivial())
            .now_or_never()
            .unwrap();

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
                async {
                    Err(MethodError::from_parts(
                    "submitSolution(uint32,uint256,address[],uint16[],uint128[],uint128[],uint16[])"
                        .to_owned(),
                    ExecutionError::Failure(Box::new(receipt)),
                ))
                }
                .boxed()
            });
        // Get objective value on old block number returns revert reason
        contract
            .expect_get_solution_objective_value()
            .with(always(), always(), eq(Some(block_number.into())))
            .return_once(move |_, _, _| {
                async{Err(anyhow!(MethodError::from_parts(
                    "submitSolution(uint32,uint256,address[],uint16[],uint128[],uint128[],uint16[])"
                        .to_owned(),
                    ExecutionError::Revert(Some("Claimed objective doesn't sufficiently improve current solution".to_owned())),
                )))}.boxed()
            });
        let mut gas_station = MockGasPriceEstimating::new();
        gas_station.expect_estimate_gas_price().return_once(|| {
            async {
                Ok(GasPrice {
                    fast: 5.into(),
                    ..Default::default()
                })
            }
            .boxed()
        });

        let submitter = StableXSolutionSubmitter::new(&contract, &gas_station);
        let result = submitter
            .submit_solution(0, Solution::trivial(), U256::zero())
            .now_or_never()
            .unwrap();

        match result.expect_err("Should have errored") {
            SolutionSubmissionError::Benign(_) => (),
            SolutionSubmissionError::Unexpected(err) => {
                panic!("Expecting benign failure, but got {}", err)
            }
        };
    }
}
