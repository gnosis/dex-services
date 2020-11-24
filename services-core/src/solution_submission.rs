mod first_match;
mod gas_price_increase;
mod gas_price_stream;
mod retry;

use crate::{
    contracts::stablex_contract::StableXContract,
    models::{BatchId, Solution},
    util::AsyncSleeping,
};
use anyhow::{anyhow, Error, Result};
use ethcontract::{
    errors::{ExecutionError, MethodError},
    jsonrpc::types::Error as RpcError,
    web3::{error::Error as Web3Error, types::TransactionReceipt},
    U256,
};
use futures::future::FutureExt as _;
use gas_estimation::GasPriceEstimating;
use retry::{RetryResult, TransactionResult, TransactionSending};
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
            .unwrap_or(SolutionSubmissionError::Unexpected(err))
    }
}

pub struct StableXSolutionSubmitter {
    contract: Arc<dyn StableXContract>,
    gas_price_estimator: Arc<dyn GasPriceEstimating>,
    async_sleep: Box<dyn AsyncSleeping>,
}

impl StableXSolutionSubmitter {
    pub fn new(
        contract: Arc<dyn StableXContract>,
        gas_price_estimator: Arc<dyn GasPriceEstimating>,
    ) -> Self {
        Self::with_estimator_and_sleep(
            contract.clone(),
            gas_price_estimator,
            crate::util::AsyncSleep {},
        )
    }
}

impl StableXSolutionSubmitter {
    fn with_estimator_and_sleep(
        contract: Arc<dyn StableXContract>,
        gas_price_estimator: Arc<dyn GasPriceEstimating>,
        async_sleep: impl AsyncSleeping,
    ) -> Self {
        Self {
            contract,
            gas_price_estimator,
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
        result: SolutionResult,
    ) -> Result<(), SolutionSubmissionError> {
        match result.0 {
            Ok(()) => Ok(()),
            Err(err) => Err(self.convert_submit_error(batch_index, solution, err).await),
        }
    }
}

#[async_trait::async_trait]
impl StableXSolutionSubmitting for StableXSolutionSubmitter {
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
        // Add some extra time in case of desync between real time and ethereum node current block time.
        let cancel_instant = target_confirm_time + Duration::from_secs(30);
        let cancel_duration = cancel_instant
            .checked_duration_since(Instant::now())
            .unwrap_or_else(|| Duration::from_secs(0));

        let solution_sender = SolutionSender {
            contract: self.contract.as_ref(),
            batch_index,
            solution: solution.clone(),
            claimed_objective_value,
            nonce,
        };
        let cancellation_sender = CancellationSender {
            contract: self.contract.as_ref(),
            nonce,
        };
        let cancel_future = async {
            self.async_sleep.sleep(cancel_duration).await;
            cancellation_sender
        };

        let stream = gas_price_stream::gas_price_stream(
            target_confirm_time,
            gas_price_cap,
            self.gas_price_estimator.as_ref(),
            self.async_sleep.as_ref(),
        );

        match retry::retry(solution_sender, cancel_future.boxed(), stream).await {
            Some(RetryResult::Submitted(result)) => {
                log::info!("solution submission transaction completed first");
                self.convert_submit_result(batch_index, solution, result)
                    .await
            }
            Some(RetryResult::Cancelled(result)) => {
                log::info!("cancel transaction completed first");
                convert_cancel_result(result)
            }
            None => {
                log::info!("transaction was never sent");
                Err(SolutionSubmissionError::Unexpected(anyhow!(
                    "transaction was never sent"
                )))
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

fn convert_cancel_result(result: CancellationResult) -> Result<(), SolutionSubmissionError> {
    let error = match result.0 {
        Ok(_) => anyhow!("solution submission transaction not confirmed in time"),
        Err(err) => Error::from(err).context("failed to cancel solution submission"),
    };
    Err(SolutionSubmissionError::Unexpected(error))
}

fn is_transaction_error(error: &ExecutionError) -> bool {
    // This is the error as we've seen it on openethereum nodes. The code and error messages can
    // be found in openethereum's source code in `rpc/src/v1/helpers/errors.rs`.
    // TODO: check how this looks on geth and infura. Not recognizing the error is not a serious
    // problem but it will make us sometimes log an error when there actually was no problem.
    matches!(error, ExecutionError::Web3(Web3Error::Rpc(RpcError { code, .. })) if code.code() == -32010)
}

struct SolutionResult(Result<(), MethodError>);
impl TransactionResult for SolutionResult {
    fn was_mined(&self) -> bool {
        if let Err(err) = &self.0 {
            !is_transaction_error(&err.inner)
        } else {
            false
        }
    }
}

struct SolutionSender<'a> {
    contract: &'a dyn StableXContract,
    batch_index: u32,
    solution: Solution,
    claimed_objective_value: U256,
    nonce: U256,
}
#[async_trait::async_trait]
impl<'a> TransactionSending for SolutionSender<'a> {
    type Output = SolutionResult;
    async fn send(&self, gas_price: f64) -> Self::Output {
        let result = self
            .contract
            .submit_solution(
                self.batch_index,
                self.solution.clone(),
                self.claimed_objective_value,
                U256::from_f64_lossy(gas_price),
                self.nonce,
            )
            .await;
        SolutionResult(result)
    }
}

struct CancellationResult(Result<(), ExecutionError>);
impl TransactionResult for CancellationResult {
    fn was_mined(&self) -> bool {
        if let Err(err) = &self.0 {
            !is_transaction_error(&err)
        } else {
            false
        }
    }
}

struct CancellationSender<'a> {
    contract: &'a dyn StableXContract,
    nonce: U256,
}
#[async_trait::async_trait]
impl<'a> TransactionSending for CancellationSender<'a> {
    type Output = CancellationResult;
    async fn send(&self, gas_price: f64) -> Self::Output {
        let result = self
            .contract
            .send_noop_transaction(U256::from_f64_lossy(gas_price), self.nonce)
            .await;
        CancellationResult(result.map(|_| ()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        contracts::stablex_contract::MockStableXContract, gas_price::MockGasPriceEstimating,
        util::MockAsyncSleeping,
    };
    use anyhow::anyhow;
    use ethcontract::{web3::types::H2048, H256};
    use futures::future;
    use mockall::predicate::{always, eq};

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

        let submitter = StableXSolutionSubmitter::with_estimator_and_sleep(
            Arc::new(contract),
            Arc::new(MockGasPriceEstimating::new()),
            MockAsyncSleeping::new(),
        );
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
        contract
            .expect_submit_solution()
            .return_once(|_, _, _, _, _| {
                immediate!(Err(MethodError::from_parts(
                "submitSolution(uint32,uint256,address[],uint16[],uint128[],uint128[],uint16[])"
                    .to_owned(),
                ExecutionError::Failure(Box::new(receipt)),
            )))
            });
        let mut gas_price = MockGasPriceEstimating::new();
        gas_price
            .expect_estimate_with_limits()
            .returning(|_, _| Ok(1.0));
        let mut sleep = MockAsyncSleeping::new();
        sleep
            .expect_sleep()
            .returning(|_| future::pending().boxed());

        let submitter = StableXSolutionSubmitter::with_estimator_and_sleep(
            Arc::new(contract),
            Arc::new(gas_price),
            sleep,
        );
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
