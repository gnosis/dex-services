mod retry;

use crate::{
    contracts::stablex_contract::StableXContract,
    gas_station::GasPriceEstimating,
    models::{BatchId, Solution},
};

use anyhow::{anyhow, Error, Result};
use async_std::future::TimeoutError;
use ethcontract::errors::{ExecutionError, MethodError};
use ethcontract::web3::types::TransactionReceipt;
use ethcontract::U256;
use futures::future::{BoxFuture, FutureExt as _};
use log::info;
use retry::SolutionTransactionSending;
use std::time::{Duration, SystemTime};
use thiserror::Error;

/// The amount of time the solution submitter should wait between polling the
/// current batch ID to wait for a block to be mined after the solving batch
/// stops accepting orders.
#[cfg(not(test))]
const POLL_TIMEOUT: Duration = Duration::from_secs(5);
#[cfg(test)]
const POLL_TIMEOUT: Duration = Duration::from_secs(0);

const GAS_PRICE_CAP: u64 = 200_000_000_000;

// openethereum requires that the gas price of the resubmitted transaction has increased by at
// least 12.5%.
const MIN_GAS_PRICE_INCREASE_FACTOR: f64 = 1.125 * (1.0 + f64::EPSILON);

#[cfg_attr(test, mockall::automock)]
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
    retry_with_gas_price_increase: Box<dyn SolutionTransactionSending + Send + Sync + 'a>,
}

impl<'a> StableXSolutionSubmitter<'a> {
    pub fn new(
        contract: &'a (dyn StableXContract + Sync),
        gas_price_estimating: &'a (dyn GasPriceEstimating + Sync),
    ) -> Self {
        Self::with_retrying(
            contract,
            retry::RetryWithGasPriceIncrease::new(contract, gas_price_estimating),
        )
    }

    fn with_retrying(
        contract: &'a (dyn StableXContract + Sync),
        retry_with_gas_price_increase: impl SolutionTransactionSending + Send + Sync + 'a,
    ) -> Self {
        Self {
            contract,
            retry_with_gas_price_increase: Box::new(retry_with_gas_price_increase),
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

    async fn handle_submit_solution_result(
        &self,
        batch_index: u32,
        solution: Solution,
        result: std::result::Result<Result<(), MethodError>, TimeoutError>,
        nonce: U256,
    ) -> Result<(), SolutionSubmissionError> {
        if let Ok(submit_result) = result {
            match submit_result {
                Ok(()) => Ok(()),
                Err(err) => Err(self.make_error(batch_index, solution, err).await),
            }
        } else {
            let gas_price =
                U256::from((GAS_PRICE_CAP as f64 * MIN_GAS_PRICE_INCREASE_FACTOR).ceil() as u128);
            log::info!(
                "cancelling transaction because it took too long, using gas price {}",
                gas_price
            );
            match self.contract.send_noop_transaction(gas_price, nonce).await {
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
            Err(SolutionSubmissionError::Unexpected(anyhow!(
                "solution submission transaction not confirmed in time"
            )))
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
            let nonce = self.contract.get_transaction_count().await?;
            let submit_future = self.retry_with_gas_price_increase.retry(retry::Args {
                batch_index,
                solution: solution.clone(),
                claimed_objective_value,
                gas_price_cap: GAS_PRICE_CAP.into(),
                nonce,
            });
            // Add some extra time in case of desync between real time and ethereum node current block time.
            let deadline = BatchId::from(batch_index).solve_end_time() + Duration::from_secs(30);
            let remaining = deadline
                .duration_since(SystemTime::now())
                .unwrap_or(Duration::from_secs(0));
            let result = async_std::future::timeout(remaining, submit_future).await;
            self.handle_submit_solution_result(batch_index, solution, result, nonce)
                .await
        }
        .boxed()
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
    use crate::contracts::stablex_contract::{MockStableXContract, NoopTransactionError};
    use retry::MockSolutionTransactionSending;

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

        let retry = MockSolutionTransactionSending::new();

        let result = {
            let submitter = StableXSolutionSubmitter::with_retrying(&contract, retry);
            submitter
                .get_solution_objective_value(0, Solution::trivial())
                .now_or_never()
                .unwrap()
        };
        contract.checkpoint();
        assert_eq!(result.unwrap(), U256::from(42));
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

        let retry = MockSolutionTransactionSending::new();

        let submitter = StableXSolutionSubmitter::with_retrying(&contract, retry);
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

        contract
            .expect_get_transaction_count()
            .returning(|| immediate!(Ok(U256::from(0))));
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

        let mut retry = MockSolutionTransactionSending::new();
        retry.expect_retry().times(1).return_once(|_| {
            immediate!(Err(MethodError::from_parts(
                "submitSolution(uint32,uint256,address[],uint16[],uint128[],uint128[],uint16[])"
                    .to_owned(),
                ExecutionError::Failure(Box::new(receipt))
            )))
        });

        let submitter = StableXSolutionSubmitter::with_retrying(&contract, retry);
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

    // Silly way to create a TimeoutError because the type can't be constructed directly.
    fn timeout_error() -> TimeoutError {
        async_std::future::timeout(Duration::from_secs(0), futures::future::pending::<()>())
            .now_or_never()
            .unwrap()
            .unwrap_err()
    }

    #[test]
    fn handle_submit_solution_result_timeout() {
        let mut contract = MockStableXContract::new();
        contract
            .expect_send_noop_transaction()
            .with(eq(U256::from(225_000_000_001u128)), eq(U256::from(0)))
            .times(1)
            // The specific error doesn't matter.
            .returning(|_, _| immediate!(Err(NoopTransactionError::NoAccount)));
        let retry = MockSolutionTransactionSending::new();
        let submitter = StableXSolutionSubmitter::with_retrying(&contract, retry);
        let result = submitter
            .handle_submit_solution_result(0, Solution::trivial(), Err(timeout_error()), 0.into())
            .now_or_never()
            .unwrap();
        assert!(result.is_err());
    }

    #[test]
    fn handle_submit_solution_result_ok() {
        let contract = MockStableXContract::new();
        let retry = MockSolutionTransactionSending::new();
        let submitter = StableXSolutionSubmitter::with_retrying(&contract, retry);
        let result = submitter
            .handle_submit_solution_result(0, Solution::trivial(), Ok(Ok(())), 0.into())
            .now_or_never()
            .unwrap();
        assert!(result.is_ok());
    }
}
