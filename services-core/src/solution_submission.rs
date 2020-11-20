mod first_match;
mod retry;

use crate::{
    contracts::stablex_contract::StableXContract,
    gas_price::GasPriceEstimating,
    models::{BatchId, Solution},
    util::AsyncSleeping,
};
use anyhow::{anyhow, Error, Result};
use ethcontract::{
    errors::{ExecutionError, MethodError},
    web3::types::TransactionReceipt,
    U256,
};
use retry::{RetryResult, SolutionTransactionSending};
use std::{
    sync::Arc,
    time::{Duration, Instant, SystemTime},
};
use thiserror::Error;

#[cfg_attr(test, mockall::automock)]
#[async_trait::async_trait]
pub trait StableXSolutionSubmitting {
    /// Return the objective value for the given solution in the given
    /// batch or an error.
    ///
    /// # Arguments
    /// * `batch_index` - the auction for which this solutions should be evaluated
    /// * `orders` - the list of orders for which this solution is applicable
    /// * `solution` - the solution to be evaluated
    async fn get_solution_objective_value(
        &self,
        batch_index: u32,
        solution: Solution,
    ) -> Result<U256, SolutionSubmissionError>;

    /// Submits the provided solution and returns the result of the submission
    ///
    /// # Arguments
    /// * `batch_index` - the auction for which this solutions should be evaluated
    /// * `orders` - the list of orders for which this solution is applicable
    /// * `solution` - the solution to be evaluated
    /// * `claimed_objective_value` - the objective value of the provided solution.
    async fn submit_solution(
        &self,
        batch_index: u32,
        solution: Solution,
        claimed_objective_value: U256,
        gas_price_cap: f64,
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
    contract: Arc<dyn StableXContract>,
    retry_with_gas_price_increase: Box<dyn SolutionTransactionSending + Send + Sync + 'a>,
    async_sleep: Box<dyn AsyncSleeping + 'a>,
}

impl<'a> StableXSolutionSubmitter<'a> {
    pub fn new(
        contract: Arc<dyn StableXContract>,
        gas_price_estimating: Arc<dyn GasPriceEstimating>,
    ) -> Self {
        Self::with_retry_and_sleep(
            contract.clone(),
            retry::RetryWithGasPriceIncrease::new(contract, gas_price_estimating),
            crate::util::AsyncSleep {},
        )
    }

    fn with_retry_and_sleep(
        contract: Arc<dyn StableXContract>,
        retry_with_gas_price_increase: impl SolutionTransactionSending + Send + Sync + 'a,
        async_sleep: impl AsyncSleeping + 'a,
    ) -> Self {
        Self {
            contract,
            retry_with_gas_price_increase: Box::new(retry_with_gas_price_increase),
            async_sleep: Box::new(async_sleep),
        }
    }

    /// Turn a method error from a solution submission into a SolutionSubmissionError.
    async fn convert_submit_error(
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

    async fn convert_submit_result(
        &self,
        batch_index: u32,
        solution: Solution,
        result: Result<(), MethodError>,
    ) -> Result<(), SolutionSubmissionError> {
        match result {
            Ok(()) => Ok(()),
            Err(err) => Err(self.convert_submit_error(batch_index, solution, err).await),
        }
    }
}

#[async_trait::async_trait]
impl<'a> StableXSolutionSubmitting for StableXSolutionSubmitter<'a> {
    async fn get_solution_objective_value(
        &self,
        batch_index: u32,
        solution: Solution,
    ) -> Result<U256, SolutionSubmissionError> {
        self.contract
            .get_solution_objective_value(batch_index, solution, None)
            .await
            .map_err(SolutionSubmissionError::from)
    }

    async fn submit_solution(
        &self,
        batch_index: u32,
        solution: Solution,
        claimed_objective_value: U256,
        gas_price_cap: f64,
    ) -> Result<(), SolutionSubmissionError> {
        let target_confirm_time = Instant::now()
            + BatchId::from(batch_index)
                .solve_end_time()
                .duration_since(SystemTime::now())
                .unwrap_or_else(|_| Duration::from_secs(0));
        let nonce = self.contract.get_transaction_count().await?;
        let args = retry::Args {
            batch_index,
            solution: solution.clone(),
            claimed_objective_value,
            gas_price_cap,
            nonce,
            target_confirm_time,
        };

        // Add some extra time in case of desync between real time and ethereum node current block time.
        let cancel_instant = target_confirm_time + Duration::from_secs(30);
        let cancel_duration = cancel_instant
            .checked_duration_since(Instant::now())
            .unwrap_or_else(|| Duration::from_secs(0));
        let cancel_future = self.async_sleep.sleep(cancel_duration);

        match self
            .retry_with_gas_price_increase
            .retry(args, cancel_future)
            .await
        {
            RetryResult::Submitted(result) => {
                log::info!("solution submission transaction completed first");
                self.convert_submit_result(batch_index, solution, result)
                    .await
            }
            RetryResult::Cancelled(result) => {
                log::info!("cancel transaction completed first");
                convert_cancel_result(result)
            }
        }
    }
}

fn extract_transaction_receipt(err: &MethodError) -> Option<&TransactionReceipt> {
    match &err.inner {
        ExecutionError::Failure(tx) => Some(tx.as_ref()),
        _ => None,
    }
}

fn convert_cancel_result(
    result: Result<(), ExecutionError>,
) -> Result<(), SolutionSubmissionError> {
    let error = match result {
        Ok(_) => anyhow!("solution submission transaction not confirmed in time"),
        Err(err) => Error::from(err).context("failed to cancel solution submission"),
    };
    Err(SolutionSubmissionError::Unexpected(error))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{contracts::stablex_contract::MockStableXContract, util::MockAsyncSleeping};
    use anyhow::anyhow;
    use ethcontract::{web3::types::H2048, H256};
    use futures::{future, FutureExt as _};
    use mockall::predicate::{always, eq};
    use retry::MockSolutionTransactionSending;

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

        let retry = MockSolutionTransactionSending::new();
        let sleep = MockAsyncSleeping::new();

        let submitter =
            StableXSolutionSubmitter::with_retry_and_sleep(Arc::new(contract), retry, sleep);
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
            root: None,
            logs_bloom: H2048::zero(),
        };

        contract
            .expect_get_transaction_count()
            .returning(|| Ok(U256::from(0)));
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

        let mut retry = MockSolutionTransactionSending::new();
        retry.expect_retry().times(1).return_once(|_, _| {
            immediate!(RetryResult::Submitted(Err(MethodError::from_parts(
                "submitSolution(uint32,uint256,address[],uint16[],uint128[],uint128[],uint16[])"
                    .to_owned(),
                ExecutionError::Failure(Box::new(receipt)),
            ))))
        });

        let mut sleep = MockAsyncSleeping::new();
        sleep
            .expect_sleep()
            .returning(|_| future::pending().boxed());

        let submitter =
            StableXSolutionSubmitter::with_retry_and_sleep(Arc::new(contract), retry, sleep);
        let result = submitter
            .submit_solution(0, Solution::trivial(), U256::zero(), 0.0)
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
