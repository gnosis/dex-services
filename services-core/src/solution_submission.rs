mod retry;

use crate::{
    contracts::stablex_contract::{NoopTransactionError, StableXContract},
    gas_price::GasPriceEstimating,
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
use futures::future::{self, Either};
use retry::SolutionTransactionSending;
use std::{
    sync::Arc,
    time::{Duration, Instant, SystemTime},
};
use thiserror::Error;

// openethereum requires that the gas price of the resubmitted transaction has increased by at
// least 12.5%.
const MIN_GAS_PRICE_INCREASE_FACTOR: f64 = 1.125 * (1.0 + f64::EPSILON);

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

    async fn cancel_transaction_after_deadline(
        &self,
        nonce: U256,
        gas_price_cap: f64,
        deadline: Instant,
    ) -> Result<(), NoopTransactionError> {
        let remaining = deadline
            .checked_duration_since(Instant::now())
            .unwrap_or_default();
        self.async_sleep.sleep(remaining).await;
        let gas_price = (gas_price_cap * MIN_GAS_PRICE_INCREASE_FACTOR).ceil();
        log::info!(
            "cancelling transaction because it took too long, using gas price {}",
            gas_price
        );
        self.contract
            .send_noop_transaction(U256::from_f64_lossy(gas_price), nonce)
            .await
            .map(|_| ())
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
        let submit_future = self.retry_with_gas_price_increase.retry(retry::Args {
            batch_index,
            solution: solution.clone(),
            claimed_objective_value,
            gas_price_cap,
            nonce,
            target_confirm_time,
        });
        // Add some extra time in case of desync between real time and ethereum node current block time.
        let deadline = target_confirm_time + Duration::from_secs(30);
        let cancel_future = self.cancel_transaction_after_deadline(nonce, gas_price_cap, deadline);

        // Run both futures at the same time. When one of them completes check whether the
        // result is a "nonce already used error". If this is the case then the other future's
        // transaction must have gone through so return that one instead.
        // We need to handle this error because exactly one of the transactions will go through
        // but we might observe the other transaction failing first.
        futures::pin_mut!(cancel_future);
        match future::select(submit_future, cancel_future).await {
            Either::Left((submit_result, cancel_future)) => {
                if submit_result.is_transaction_error() {
                    log::info!("solution submission transaction is nonce error");
                    Err(convert_cancel_result(cancel_future.await))
                } else {
                    log::info!("solution submission transaction completed first");
                    self.convert_submit_result(batch_index, solution, submit_result)
                        .await
                }
            }
            Either::Right((cancel_result, submit_future)) => {
                if cancel_result.is_transaction_error() {
                    log::info!("cancel transaction is nonce error");
                    self.convert_submit_result(batch_index, solution, submit_future.await)
                        .await
                } else {
                    log::info!("cancel transaction completed first");
                    Err(convert_cancel_result(cancel_result))
                }
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

fn convert_cancel_result(result: Result<(), NoopTransactionError>) -> SolutionSubmissionError {
    match result {
        Ok(()) => SolutionSubmissionError::Unexpected(anyhow!(
            "solution submission transaction not confirmed in time"
        )),
        Err(err) => SolutionSubmissionError::Unexpected(
            Error::from(err).context("failed to cancel solution submission"),
        ),
    }
}

trait IsOpenEthereumTransactionError {
    /// Is this an error with the transaction itself instead of an evm related error.
    fn is_transaction_error(&self) -> bool;
}

impl IsOpenEthereumTransactionError for ExecutionError {
    fn is_transaction_error(&self) -> bool {
        // This is the error as we've seen it on openethereum nodes. The code and error messages can
        // be found in openethereum's source code in `rpc/src/v1/helpers/errors.rs`.
        // TODO: check how this looks on geth and infura. Not recognizing the error is not a serious
        // problem but it will make us sometimes log an error when there actually was no problem.
        matches!(self, ExecutionError::Web3(Web3Error::Rpc(RpcError { code, .. })) if code.code() == -32010)
    }
}

impl IsOpenEthereumTransactionError for Result<(), MethodError> {
    fn is_transaction_error(&self) -> bool {
        match self {
            Ok(()) => false,
            Err(MethodError { inner, .. }) => inner.is_transaction_error(),
        }
    }
}

impl IsOpenEthereumTransactionError for Result<(), NoopTransactionError> {
    fn is_transaction_error(&self) -> bool {
        match self {
            Err(NoopTransactionError::ExecutionError(err)) => err.is_transaction_error(),
            _ => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        contracts::stablex_contract::{MockStableXContract, NoopTransactionError},
        util::{FutureWaitExt as _, MockAsyncSleeping},
    };
    use anyhow::anyhow;
    use ethcontract::{web3::types::H2048, H256};
    use futures::FutureExt as _;
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
        retry.expect_retry().times(1).return_once(|_| {
            async move {
                Err(MethodError::from_parts(
                "submitSolution(uint32,uint256,address[],uint16[],uint128[],uint128[],uint16[])"
                    .to_owned(),
                ExecutionError::Failure(Box::new(receipt)),
            ))
            }
            .boxed()
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

    #[test]
    fn submit_timeout_results_in_cancellation() {
        let mut contract = MockStableXContract::new();
        let mut retry = MockSolutionTransactionSending::new();
        let mut sleep = MockAsyncSleeping::new();

        contract
            .expect_get_transaction_count()
            .returning(|| Ok(U256::from(0)));
        retry
            .expect_retry()
            .returning(|_| future::pending().boxed());
        sleep
            .expect_sleep()
            .returning(|_| future::ready(()).boxed());
        contract
            .expect_send_noop_transaction()
            .times(1)
            .returning(|_, _| immediate!(Err(NoopTransactionError::NoAccount)));

        let submitter =
            StableXSolutionSubmitter::with_retry_and_sleep(Arc::new(contract), retry, sleep);
        let result = submitter
            .submit_solution(0, Solution::trivial(), 0.into(), 0.into())
            .now_or_never()
            .unwrap();
        assert!(result.is_err());
    }

    #[test]
    fn submission_completes_during_cancellation() {
        let (sender, receiver) = futures::channel::oneshot::channel();
        let mut contract = MockStableXContract::new();
        let mut retry = MockSolutionTransactionSending::new();
        let mut async_sleep = MockAsyncSleeping::new();

        contract
            .expect_get_transaction_count()
            .returning(|| Ok(U256::from(0)));
        retry.expect_retry().return_once(|_| {
            async move {
                receiver.await.unwrap();
                Ok(())
            }
            .boxed()
        });
        async_sleep.expect_sleep().returning(|_| immediate!(()));
        contract
            .expect_send_noop_transaction()
            .times(1)
            .return_once(move |_, _| {
                sender.send(()).unwrap();
                futures::future::pending().boxed()
            });

        let submitter =
            StableXSolutionSubmitter::with_retry_and_sleep(Arc::new(contract), retry, async_sleep);
        let result = submitter
            .submit_solution(0, Solution::trivial(), 0.into(), 0.into())
            .wait();
        assert!(result.is_ok());
    }

    pub fn nonce_error() -> ExecutionError {
        ExecutionError::Web3(Web3Error::Rpc(RpcError {
            code: ethcontract::jsonrpc::types::ErrorCode::ServerError(-32010),
            message: "Transaction nonce is too low.".to_string(),
            data: None,
        }))
    }

    #[test]
    fn cancellation_fails_with_nonce_error_before_submission_completes() {
        let (sender, receiver) = futures::channel::oneshot::channel();
        let mut contract = MockStableXContract::new();
        let mut retry = MockSolutionTransactionSending::new();
        let mut async_sleep = MockAsyncSleeping::new();

        contract
            .expect_get_transaction_count()
            .returning(|| Ok(U256::from(0)));
        retry.expect_retry().return_once(|_| {
            async move {
                receiver.await.unwrap();
                Ok(())
            }
            .boxed()
        });
        async_sleep.expect_sleep().returning(|_| immediate!(()));
        contract
            .expect_send_noop_transaction()
            .times(1)
            .return_once(move |_, _| {
                sender.send(()).unwrap();
                immediate!(Err(nonce_error().into()))
            });

        let submitter =
            StableXSolutionSubmitter::with_retry_and_sleep(Arc::new(contract), retry, async_sleep);
        let result = submitter
            .submit_solution(0, Solution::trivial(), 0.into(), 0.into())
            .wait();
        assert!(result.is_ok());
    }
}
