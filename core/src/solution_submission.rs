#![allow(clippy::ptr_arg)] // required for automock

use crate::contracts::stablex_contract::StableXContract;
use crate::gas_station::GasPriceEstimating;
use crate::models::{BatchId, Solution};

use anyhow::{anyhow, Error, Result};
use ethcontract::errors::{ExecutionError, MethodError};
use ethcontract::web3::types::TransactionReceipt;
use ethcontract::U256;
use futures::future::{self, BoxFuture, Either, FutureExt as _};
#[cfg(test)]
use mockall::automock;
use std::time::{Duration, SystemTime};
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
        err: RetryError,
    ) -> SolutionSubmissionError {
        match err {
            RetryError::MethodError(err) => {
                if let Some(tx) = extract_transaction_receipt(&err) {
                    if let Some(block_number) = tx.block_number {
                        if let Err(err) = self
                            .contract
                            .get_solution_objective_value(
                                batch_index,
                                solution,
                                Some(block_number.into()),
                            )
                            .await
                        {
                            return SolutionSubmissionError::from(err);
                        }
                    }
                }
                SolutionSubmissionError::Unexpected(err.into())
            }
            RetryError::TransactionNotConfirmedInTime => SolutionSubmissionError::Unexpected(
                anyhow!("solution submission transaction not confirmed in time"),
            ),
        }
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
                log::info!("Solved batch is not yet accepting solutions, waiting for next batch.");
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

fn is_confirm_timeout(result: &Result<(), MethodError>) -> bool {
    matches!(
        result,
        &Err(MethodError {
            inner: ExecutionError::ConfirmTimeout,
            ..
        })
    )
}

async fn wait_for_deadline(deadline: SystemTime) {
    if let Ok(remaining) = deadline.duration_since(SystemTime::now()) {
        async_std::task::sleep(remaining).await
    }
}

struct InfallibleGasPriceEstimator<'a> {
    gas_price_estimating: &'a (dyn GasPriceEstimating + Sync),
    previous_gas_price: U256,
}

impl<'a> InfallibleGasPriceEstimator<'a> {
    fn new(
        gas_price_estimating: &'a (dyn GasPriceEstimating + Sync),
        default_gas_price: U256,
    ) -> Self {
        Self {
            gas_price_estimating,
            previous_gas_price: default_gas_price,
        }
    }

    /// Get a fresh price estimate or if that fails return the most recent previous result.
    async fn estimate(&mut self) -> U256 {
        match self.gas_price_estimating.estimate_gas_price().await {
            Ok(gas_estimate) => {
                self.previous_gas_price = gas_estimate.fast;
            }
            Err(ref err) => {
                log::warn!(
                    "failed to get gas price from gnosis safe gas station: {}",
                    err
                );
            }
        };
        self.previous_gas_price
    }
}

fn gas_price(estimated_price: U256, price_increase_count: u32, cap: U256) -> U256 {
    let factor = 2u32.pow(price_increase_count);
    cap.min(estimated_price * factor)
}

#[derive(Debug)]
enum RetryError {
    MethodError(MethodError),
    /// The batch solution acceptance period ended before we got confirmation that the transaction
    /// was mined. Can happen if gas prices in the network are high.
    TransactionNotConfirmedInTime,
}

/// Keep retrying to submit the solution with higher gas prices until the transaction completes or
/// the gas cap or the deadline is reached.
/// If the transaction did not complete because it did not get enough block confirmations the
/// transaction is overriden with a noop transaction.
async fn retry_with_gas_price_increase(
    contract: &(dyn StableXContract + Sync),
    batch_index: u32,
    solution: Solution,
    claimed_objective_value: U256,
    gas_price_estimating: &(dyn GasPriceEstimating + Sync),
    gas_price_cap: U256,
) -> std::result::Result<(), RetryError> {
    const BLOCK_TIMEOUT: usize = 2;
    const DEFAULT_GAS_PRICE: u64 = 15_000_000_000;
    // openethereum requires that the gas price of the resubmitted transaction has increased by at
    // least 12.5%.
    const MIN_GAS_PRICE_INCREASE_FACTOR: f64 = 1.125 * (1.0 + f64::EPSILON);

    let effective_gas_price_cap = U256::from(
        (gas_price_cap.as_u128() as f64 / MIN_GAS_PRICE_INCREASE_FACTOR).floor() as u128,
    );
    assert!(effective_gas_price_cap <= gas_price_cap);

    let mut gas_price_estimator =
        InfallibleGasPriceEstimator::new(gas_price_estimating, DEFAULT_GAS_PRICE.into());

    // Add some extra time in case of desync between real time and ethereum node current block time.
    let deadline = BatchId::from(batch_index).solve_end_time() + Duration::from_secs(30);
    for gas_price_increase_count in 0u32.. {
        let estimated_price = gas_price_estimator.estimate().await;
        let gas_price = gas_price(estimated_price, gas_price_increase_count, gas_price_cap);
        assert!(gas_price <= gas_price_cap);
        log::info!(
            "solution submission try {} with gas price {}",
            gas_price_increase_count,
            gas_price
        );
        let is_last_iteration = gas_price >= effective_gas_price_cap;
        // In the last iteration there is no block timeout. This is important for the deadline
        // logic because we assume that in the last iteration the solution future will never be
        // a confirm timeout so that it either completes in a way we don't need to cancel or we hit
        // the deadline.
        let block_timeout = if is_last_iteration {
            None
        } else {
            Some(BLOCK_TIMEOUT)
        };
        let solution_future = contract.submit_solution(
            batch_index,
            solution.clone(),
            claimed_objective_value,
            gas_price,
            block_timeout,
        );
        let deadline_future = wait_for_deadline(deadline);
        futures::pin_mut!(deadline_future);
        let select_future = future::select(solution_future, deadline_future);
        match determine_retry_action(select_future.await, is_last_iteration) {
            RetryAction::IncreaseGasPrice => (),
            RetryAction::Cancel => {
                let gas_price = U256::from(
                    (gas_price.as_u128() as f64 * MIN_GAS_PRICE_INCREASE_FACTOR).ceil() as u128,
                );
                match contract.send_noop_transaction(gas_price).await {
                    Ok(_) => log::info!(
                        "cancelled solution submission of batch {} because of deadline",
                        batch_index
                    ),
                    Err(err) => log::error!(
                        "failed to cancel solution submission of batch {} after deadline: {:?}",
                        batch_index,
                        err
                    ),
                }
                return Err(RetryError::TransactionNotConfirmedInTime);
            }
            RetryAction::Done(result) => return result,
        }
    }
    unreachable!("increased gas price past expected limit");
}

#[derive(Debug)]
enum RetryAction {
    IncreaseGasPrice,
    Cancel,
    Done(std::result::Result<(), RetryError>),
}

// Either::Left is the solution submission future, Either::Right the deadline future.
fn determine_retry_action<T0, T1>(
    either: Either<(Result<(), MethodError>, T0), ((), T1)>,
    is_last_iteration: bool,
) -> RetryAction {
    match either {
        Either::Left((result, _)) => match (is_confirm_timeout(&result), is_last_iteration) {
            (true, false) => RetryAction::IncreaseGasPrice,
            (true, true) => {
                // In the last iteration we set the block timeout to None so we expect to never get
                // a timeout.
                log::warn!("unexpected confirm timeout");
                RetryAction::Cancel
            }
            (false, _) => RetryAction::Done(result.map_err(RetryError::MethodError)),
        },
        Either::Right(_) => RetryAction::Cancel,
    }
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
    use ethcontract::{web3::types::H2048, H256};
    use mockall::predicate::{always, eq};

    #[test]
    fn infallible_gas_price_estimator_uses_default_and_previous_result() {
        let mut gas_station = MockGasPriceEstimating::new();
        gas_station
            .expect_estimate_gas_price()
            .times(1)
            .return_once(|| immediate!(Err(anyhow!(""))));
        gas_station
            .expect_estimate_gas_price()
            .times(1)
            .return_once(|| {
                immediate!(Ok(GasPrice {
                    fast: 5.into(),
                    ..Default::default()
                }))
            });
        gas_station
            .expect_estimate_gas_price()
            .times(1)
            .return_once(|| {
                immediate!(Ok(GasPrice {
                    fast: 6.into(),
                    ..Default::default()
                }))
            });
        gas_station
            .expect_estimate_gas_price()
            .times(1)
            .return_once(|| immediate!(Err(anyhow!(""))));

        let mut estimator = InfallibleGasPriceEstimator::new(&gas_station, 3.into());
        assert_eq!(estimator.estimate().now_or_never().unwrap(), U256::from(3));
        assert_eq!(estimator.estimate().now_or_never().unwrap(), U256::from(5));
        assert_eq!(estimator.estimate().now_or_never().unwrap(), U256::from(6));
        assert_eq!(estimator.estimate().now_or_never().unwrap(), U256::from(6));
    }

    #[test]
    fn gas_price_increases_as_expected_and_hits_limit() {
        let estimated = U256::from(5);
        let cap = U256::from(50);
        assert_eq!(gas_price(estimated, 0, cap), U256::from(5));
        assert_eq!(gas_price(estimated, 1, cap), U256::from(10));
        assert_eq!(gas_price(estimated, 2, cap), U256::from(20));
        assert_eq!(gas_price(estimated, 3, cap), U256::from(40));
        assert_eq!(gas_price(estimated, 4, cap), U256::from(50));
        assert_eq!(gas_price(estimated, 5, cap), U256::from(50));
    }

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
            .times(3)
            .returning(|_, _, _, _, _| {
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
            .times(3)
            .returning(|| {
                async {
                    Ok(GasPrice {
                        fast: 5.into(),
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
