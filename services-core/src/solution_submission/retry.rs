use super::first_match::FirstMatchOrLast;
use crate::util::{self, AsyncSleeping, Now};
use crate::{
    contracts::stablex_contract::{StableXContract, SOLUTION_SUBMISSION_GAS_LIMIT},
    models::Solution,
};
use anyhow::Result;
use ethcontract::{
    errors::{ExecutionError, MethodError},
    jsonrpc::types::Error as RpcError,
    web3::error::Error as Web3Error,
    U256,
};
use futures::{
    future::{BoxFuture, FutureExt as _},
    stream::{self, FusedStream, StreamExt},
};
use gas_estimation::GasPriceEstimating;
use std::{
    future::Future,
    sync::Arc,
    time::{Duration, Instant},
};

// openethereum requires that the gas price of the resubmitted transaction has increased by at
// least 12.5%.
const MIN_GAS_PRICE_INCREASE_FACTOR: f64 = 1.125 * (1.0 + f64::EPSILON);

const GAS_PRICE_REFRESH_INTERVAL: Duration = Duration::from_secs(15);

pub struct Args {
    pub batch_index: u32,
    pub solution: Solution,
    pub claimed_objective_value: U256,
    pub gas_price_cap: f64,
    pub nonce: U256,
    pub target_confirm_time: Instant,
}

#[derive(Debug)]
pub enum RetryResult {
    Submitted(Result<(), MethodError>),
    Cancelled(Result<(), ExecutionError>),
}

#[cfg_attr(test, mockall::automock)]
pub trait SolutionTransactionSending {
    /// Submit the solution with an appropriate gas price based on target_confirm_time. Until the
    /// transaction has been confirmed the gas price is continually updated.
    /// When cancel_after is ready the transaction will be cancelled by sending a noop transaction
    /// at a higher gas price.
    fn retry<'a>(
        &'a self,
        args: Args,
        cancel_after: BoxFuture<'static, ()>,
    ) -> BoxFuture<'a, RetryResult>;
}

pub struct RetryWithGasPriceIncrease<'a> {
    contract: Arc<dyn StableXContract>,
    gas_price_estimating: Arc<dyn GasPriceEstimating>,
    async_sleep: Box<dyn AsyncSleeping + 'a>,
    now: Box<dyn Now + 'a>,
}

impl<'a> RetryWithGasPriceIncrease<'a> {
    pub fn new(
        contract: Arc<dyn StableXContract>,
        gas_price_estimating: Arc<dyn GasPriceEstimating>,
    ) -> Self {
        Self::with_sleep_and_now(
            contract,
            gas_price_estimating,
            util::AsyncSleep {},
            util::default_now(),
        )
    }

    pub fn with_sleep_and_now(
        contract: Arc<dyn StableXContract>,
        gas_price_estimating: Arc<dyn GasPriceEstimating>,
        async_sleep: impl AsyncSleeping + 'a,
        now: impl Now + 'a,
    ) -> Self {
        Self {
            contract,
            gas_price_estimating,
            async_sleep: Box::new(async_sleep),
            now: Box::new(now),
        }
    }
}

impl<'a> SolutionTransactionSending for RetryWithGasPriceIncrease<'a> {
    fn retry<'b>(
        &'b self,
        args: Args,
        cancel_after: BoxFuture<'b, ()>,
    ) -> BoxFuture<'b, RetryResult> {
        self.retry_(args, cancel_after).boxed()
    }
}

fn new_gas_price_estimate(
    previous_gas_price: f64,
    new_gas_price: f64,
    max_gas_price: f64,
) -> Option<f64> {
    let min_gas_price = previous_gas_price * MIN_GAS_PRICE_INCREASE_FACTOR;
    if min_gas_price > max_gas_price {
        return None;
    }
    if new_gas_price <= previous_gas_price {
        // Gas price has not increased.
        return None;
    }
    // Gas price could have increased but doesn't respect minimum increase so adjust it up.
    let new_price = min_gas_price.max(new_gas_price);
    Some(new_price.min(max_gas_price))
}

impl<'a> RetryWithGasPriceIncrease<'a> {
    async fn gas_price(&self, target_confirm_time: Instant) -> Result<f64> {
        let time_remaining = target_confirm_time.saturating_duration_since(self.now.instant_now());
        // TODO: Use a more accurate gas limit once the gas estimators take that into account.
        self.gas_price_estimating
            .estimate_with_limits(SOLUTION_SUBMISSION_GAS_LIMIT as f64, time_remaining)
            .await
    }

    async fn submit_solution(&self, args: &Args, gas_price: f64) -> RetryResult {
        RetryResult::Submitted(
            self.contract
                .submit_solution(
                    args.batch_index,
                    args.solution.clone(),
                    args.claimed_objective_value,
                    U256::from_f64_lossy(gas_price),
                    args.nonce,
                )
                .await,
        )
    }

    async fn cancel(&self, gas_price: f64, nonce: U256) -> RetryResult {
        let gas_price = U256::from_f64_lossy(gas_price);
        log::debug!("cancelling transaction with gas price {}", gas_price);
        let result = self.contract.send_noop_transaction(gas_price, nonce).await;
        RetryResult::Cancelled(result.map(|_| ()))
    }

    // Yields the current gas price immediately and then every refresh interval.
    fn gas_price_stream(
        &self,
        target_confirm_time: Instant,
    ) -> impl FusedStream<Item = Result<f64>> + '_ {
        stream::unfold(true, move |first_call| async move {
            if !first_call {
                self.async_sleep.sleep(GAS_PRICE_REFRESH_INTERVAL).await;
            }
            return Some((self.gas_price(target_confirm_time).await, false));
        })
    }

    async fn retry_(&self, args: Args, cancel_after: impl Future) -> RetryResult {
        log::debug!("starting retry with gas price cap {}", args.gas_price_cap);

        let gas_price_stream = self.gas_price_stream(args.target_confirm_time);
        // make useable in `select!`
        let cancel_after = cancel_after.fuse();
        futures::pin_mut!(cancel_after);
        futures::pin_mut!(gas_price_stream);

        // This struct keeps track of all the solution and cancellation futures. If we get a
        // "nonce already used error" we continue running the other futures. We need to handle this
        // case because we do not know which transactions will complete or fail or in which order we
        // observe completion.
        let mut first_match =
            FirstMatchOrLast::new(|result: &RetryResult| !is_transaction_error(result));
        let mut last_used_gas_price = 0.0;
        loop {
            futures::select! {
                // Unwrap because the stream never ends.
                gas_price = gas_price_stream.next() => match gas_price.unwrap() {
                    Ok(gas_price) => {
                        log::debug!("estimated gas price {}", gas_price);
                        if let Some(mut gas_price) = new_gas_price_estimate(
                            last_used_gas_price,
                            gas_price,
                            args.gas_price_cap,
                        ) {
                            gas_price = gas_price.ceil();
                            last_used_gas_price = gas_price;
                            log::info!(
                                "submitting solution transaction at gas price {}",
                                gas_price
                            );
                            first_match.add(self.submit_solution(&args, gas_price).boxed());
                        }
                    }
                    Err(err) => log::error!("gas price estimation failed: {:?}", err),
                },
                result = first_match => return result,
                _ = cancel_after => break,
            }
        }

        let never_submitted_solution = last_used_gas_price == 0.0;
        if never_submitted_solution {
            return RetryResult::Cancelled(Ok(()));
        }
        let gas_price = (last_used_gas_price * MIN_GAS_PRICE_INCREASE_FACTOR).ceil();
        first_match.add(self.cancel(gas_price, args.nonce).boxed());
        first_match.await
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

fn is_transaction_error(result: &RetryResult) -> bool {
    match result {
        RetryResult::Submitted(Err(err)) => err.inner.is_transaction_error(),
        RetryResult::Cancelled(Err(err)) => err.is_transaction_error(),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        contracts::stablex_contract::MockStableXContract,
        gas_price::MockGasPriceEstimating,
        util::{FutureWaitExt as _, MockAsyncSleeping},
    };
    use ethcontract::{transaction::TransactionResult, H256};
    use futures::future;

    pub fn nonce_execution_error() -> ExecutionError {
        ExecutionError::Web3(Web3Error::Rpc(RpcError {
            code: ethcontract::jsonrpc::types::ErrorCode::ServerError(-32010),
            message: "Transaction nonce is too low.".to_string(),
            data: None,
        }))
    }

    fn nonce_method_error() -> MethodError {
        MethodError {
            signature: String::new(),
            inner: nonce_execution_error(),
        }
    }

    #[test]
    fn new_gas_price_estimate_() {
        // new below previous
        assert_eq!(new_gas_price_estimate(1.0, 0.0, 2.0), None);
        //new equal to previous
        assert_eq!(new_gas_price_estimate(1.0, 1.0, 2.0), None);
        // between previous and min increase rounded up to min increase
        assert_eq!(
            new_gas_price_estimate(1.0, 1.1, 2.0),
            Some(MIN_GAS_PRICE_INCREASE_FACTOR)
        );
        // between min increase and max stays same
        assert_eq!(new_gas_price_estimate(1.0, 1.2, 2.0), Some(1.2));
        // larger than max stays max
        assert_eq!(new_gas_price_estimate(1.0, 2.1, 2.0), Some(2.0));
        // cannot increase by min increase
        assert_eq!(new_gas_price_estimate(1.9, 1.8, 2.0), None);
        assert_eq!(new_gas_price_estimate(1.9, 1.9, 2.0), None);
        assert_eq!(new_gas_price_estimate(1.9, 1.95, 2.0), None);
        assert_eq!(new_gas_price_estimate(1.9, 2.0, 2.0), None);
        assert_eq!(new_gas_price_estimate(1.9, 2.5, 2.0), None);
    }

    #[test]
    fn respects_minimum_gas_price_increase() {
        let mut contract = MockStableXContract::new();
        let mut gas_price = MockGasPriceEstimating::new();
        let mut sleep = MockAsyncSleeping::new();
        let (sender, receiver) = futures::channel::oneshot::channel();

        gas_price
            .expect_estimate_with_limits()
            .times(1)
            .returning(|_, _| Ok(1.0));
        contract
            .expect_submit_solution()
            .times(1)
            .return_once(|_, _, _, _, _| {
                async move {
                    receiver.await.unwrap();
                    Ok(())
                }
                .boxed()
            });
        sleep.expect_sleep().times(1).returning(|_| immediate!(()));
        gas_price
            .expect_estimate_with_limits()
            .times(1)
            .return_once(move |_, _| {
                sender.send(()).unwrap();
                Ok(1.0) // gas price hasn't increased
            });
        // submit_solution isn't called again
        sleep
            .expect_sleep()
            .returning(|_| future::pending().boxed());

        let args = Args {
            batch_index: 1,
            solution: Solution::trivial(),
            claimed_objective_value: 1.into(),
            gas_price_cap: 10.0,
            nonce: 0.into(),
            target_confirm_time: Instant::now(),
        };
        let retry = RetryWithGasPriceIncrease::with_sleep_and_now(
            Arc::new(contract),
            Arc::new(gas_price),
            sleep,
            util::default_now(),
        );
        let result = retry.retry(args, future::pending().boxed()).wait();
        assert!(matches!(result, RetryResult::Submitted(Ok(()))));
    }

    #[test]
    fn nonce_error_ignored() {
        let mut contract = MockStableXContract::new();
        let mut gas_price = MockGasPriceEstimating::new();
        let mut sleep = MockAsyncSleeping::new();
        let (sender, receiver) = futures::channel::oneshot::channel();

        gas_price
            .expect_estimate_with_limits()
            .times(1)
            .returning(|_, _| Ok(1.0));
        contract
            .expect_submit_solution()
            .times(1)
            .return_once(|_, _, _, _, _| {
                async move {
                    receiver.await.unwrap();
                    Ok(())
                }
                .boxed()
            });
        sleep.expect_sleep().times(1).returning(|_| immediate!(()));
        gas_price
            .expect_estimate_with_limits()
            .times(1)
            .returning(|_, _| Ok(2.0));
        contract
            .expect_submit_solution()
            .times(1)
            .return_once(|_, _, _, _, _| {
                sender.send(()).unwrap();
                immediate!(Err(nonce_method_error()))
            });
        sleep
            .expect_sleep()
            .returning(|_| future::pending().boxed());

        let args = Args {
            batch_index: 1,
            solution: Solution::trivial(),
            claimed_objective_value: 1.into(),
            gas_price_cap: 10.0,
            nonce: 0.into(),
            target_confirm_time: Instant::now(),
        };
        let retry = RetryWithGasPriceIncrease::with_sleep_and_now(
            Arc::new(contract),
            Arc::new(gas_price),
            sleep,
            util::default_now(),
        );
        let result = retry.retry(args, future::pending().boxed()).wait();
        assert!(matches!(dbg!(result), RetryResult::Submitted(Ok(()))));
    }

    #[test]
    fn submission_completes_during_cancellation() {
        let (cancel_sender, cancel_receiver) = futures::channel::oneshot::channel();
        let (submit_sender, submit_receiver) = futures::channel::oneshot::channel();
        let mut contract = MockStableXContract::new();
        let mut gas_price = MockGasPriceEstimating::new();
        let mut sleep = MockAsyncSleeping::new();

        let cancel_future = async move {
            cancel_receiver.await.unwrap();
            submit_sender.send(()).unwrap();
        }
        .boxed();
        gas_price
            .expect_estimate_with_limits()
            .times(1)
            .returning(|_, _| Ok(1.0));
        contract
            .expect_submit_solution()
            .times(1)
            .return_once(|_, _, _, _, _| {
                async move {
                    cancel_sender.send(()).unwrap();
                    submit_receiver.await.unwrap();
                    Ok(())
                }
                .boxed()
            });
        sleep
            .expect_sleep()
            .return_once(|_| future::pending().boxed());

        let args = Args {
            batch_index: 1,
            solution: Solution::trivial(),
            claimed_objective_value: 1.into(),
            gas_price_cap: 10.0,
            nonce: 0.into(),
            target_confirm_time: Instant::now(),
        };
        let retry = RetryWithGasPriceIncrease::with_sleep_and_now(
            Arc::new(contract),
            Arc::new(gas_price),
            sleep,
            util::default_now(),
        );
        let result = retry.retry(args, cancel_future).wait();
        assert!(matches!(result, RetryResult::Submitted(Ok(()))));
    }

    #[test]
    fn cancellation_completes() {
        let (cancel_sender, cancel_receiver) = futures::channel::oneshot::channel();
        let mut contract = MockStableXContract::new();
        let mut gas_price = MockGasPriceEstimating::new();
        let mut sleep = MockAsyncSleeping::new();

        let cancel_future = async move {
            cancel_receiver.await.unwrap();
        }
        .boxed();
        gas_price
            .expect_estimate_with_limits()
            .times(1)
            .returning(|_, _| Ok(1.0));
        contract
            .expect_submit_solution()
            .times(1)
            .return_once(|_, _, _, _, _| {
                cancel_sender.send(()).unwrap();
                future::pending().boxed()
            });
        sleep
            .expect_sleep()
            .return_once(|_| future::pending().boxed());
        contract
            .expect_send_noop_transaction()
            .times(1)
            .return_once(|_, _| immediate!(Ok(TransactionResult::Hash(H256::zero()))));

        let args = Args {
            batch_index: 1,
            solution: Solution::trivial(),
            claimed_objective_value: 1.into(),
            gas_price_cap: 10.0,
            nonce: 0.into(),
            target_confirm_time: Instant::now(),
        };
        let retry = RetryWithGasPriceIncrease::with_sleep_and_now(
            Arc::new(contract),
            Arc::new(gas_price),
            sleep,
            util::default_now(),
        );
        let result = retry.retry(args, cancel_future).wait();
        assert!(matches!(result, RetryResult::Cancelled(Ok(()))));
    }
}
