use super::IsOpenEthereumTransactionError as _;
use crate::util::AsyncSleeping;
use crate::{
    contracts::stablex_contract::StableXContract, gas_price::GasPriceEstimating, models::Solution,
};
use anyhow::Result;
use ethcontract::{errors::MethodError, U256};
use futures::{
    future::{BoxFuture, FutureExt as _},
    stream::{futures_unordered::FuturesUnordered, StreamExt as _},
};
use pricegraph::num;
use std::{sync::Arc, time::Duration};

use super::MIN_GAS_PRICE_INCREASE_FACTOR;

const DEFAULT_GAS_PRICE: f64 = 15_000_000_000.0;
const SINGLE_ATTEMPT_TIMEOUT: Duration = Duration::from_secs(30);

pub struct Args {
    pub batch_index: u32,
    pub solution: Solution,
    pub claimed_objective_value: U256,
    pub gas_price_cap: f64,
    pub nonce: U256,
}

#[cfg_attr(test, mockall::automock)]
pub trait SolutionTransactionSending {
    fn retry<'a>(&'a self, args: Args) -> BoxFuture<'a, Result<(), MethodError>>;
}

pub struct RetryWithGasPriceIncrease<'a> {
    contract: Arc<dyn StableXContract>,
    gas_price_estimating: Arc<dyn GasPriceEstimating + Send + Sync>,
    async_sleep: Box<dyn AsyncSleeping + 'a>,
}

impl<'a> RetryWithGasPriceIncrease<'a> {
    pub fn new(
        contract: Arc<dyn StableXContract>,
        gas_price_estimating: Arc<dyn GasPriceEstimating + Send + Sync>,
    ) -> Self {
        Self::with_sleep(contract, gas_price_estimating, crate::util::AsyncSleep {})
    }

    pub fn with_sleep(
        contract: Arc<dyn StableXContract>,
        gas_price_estimating: Arc<dyn GasPriceEstimating + Send + Sync>,
        async_sleep: impl AsyncSleeping + 'a,
    ) -> Self {
        Self {
            contract,
            gas_price_estimating,
            async_sleep: Box::new(async_sleep),
        }
    }
}

impl<'a> SolutionTransactionSending for RetryWithGasPriceIncrease<'a> {
    fn retry<'b>(&'b self, args: Args) -> BoxFuture<'b, Result<(), MethodError>> {
        retry(
            self.contract.as_ref(),
            self.gas_price_estimating.as_ref(),
            self.async_sleep.as_ref(),
            args,
        )
        .boxed()
    }
}

struct InfallibleGasPriceEstimator<'a> {
    gas_price_estimating: &'a (dyn GasPriceEstimating + Sync),
    previous_gas_price: f64,
}

impl<'a> InfallibleGasPriceEstimator<'a> {
    fn new(
        gas_price_estimating: &'a (dyn GasPriceEstimating + Sync),
        default_gas_price: f64,
    ) -> Self {
        Self {
            gas_price_estimating,
            previous_gas_price: default_gas_price,
        }
    }

    /// Get a fresh price estimate or if that fails return the most recent previous result.
    async fn estimate(&mut self) -> f64 {
        match self.gas_price_estimating.estimate().await {
            Ok(gas_estimate) => {
                // `retry` relies on the gas price always increasing.
                self.previous_gas_price = self.previous_gas_price.max(gas_estimate);
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

fn gas_price(estimated_price: f64, price_increase_count: u32, cap: f64) -> f64 {
    let factor = 1.5f64.powf(price_increase_count as f64);
    let new_price = estimated_price * factor;
    cap.min(new_price)
}

enum FutureOutput {
    SolutionSubmission(Result<(), MethodError>),
    Timeout,
}

async fn retry(
    contract: &dyn StableXContract,
    gas_price_estimating: &(dyn GasPriceEstimating + Sync),
    async_sleep: &dyn AsyncSleeping,
    Args {
        batch_index,
        solution,
        claimed_objective_value,
        gas_price_cap,
        nonce,
    }: Args,
) -> Result<(), MethodError> {
    let effective_gas_price_cap = (gas_price_cap / MIN_GAS_PRICE_INCREASE_FACTOR).floor();
    assert!(effective_gas_price_cap <= gas_price_cap);
    let mut gas_price_estimator =
        InfallibleGasPriceEstimator::new(gas_price_estimating, DEFAULT_GAS_PRICE);
    let mut futures = FuturesUnordered::new();

    for gas_price_increase_count in 0u32.. {
        let estimated_price = gas_price_estimator.estimate().await;
        let gas_price = gas_price(estimated_price, gas_price_increase_count, gas_price_cap);
        assert!(gas_price <= gas_price_cap);
        log::info!(
            "solution submission try {} with gas price {}",
            gas_price_increase_count,
            gas_price
        );

        let solution_submission_future = contract
            .submit_solution(
                batch_index,
                solution.clone(),
                claimed_objective_value,
                num::f64_to_u256(gas_price),
                nonce,
            )
            .map(FutureOutput::SolutionSubmission);
        futures.push(solution_submission_future.boxed());

        let is_last_iteration = gas_price >= effective_gas_price_cap;
        if !is_last_iteration {
            let timeout_future = async_sleep
                .sleep(SINGLE_ATTEMPT_TIMEOUT)
                .map(|()| FutureOutput::Timeout);
            futures.push(timeout_future.boxed());
        }

        // Like in `StableXSolutionSubmitter::submit_solution` we need to handle the situation where
        // we observe a nonce error from one future before the completion of another. It is also
        // possible that a previous submission transaction completes first instead of the most
        // recent one.
        // Unwrap because we always add the solution future above and every iteration here checks
        // that there are still futures left.
        while let FutureOutput::SolutionSubmission(result) = futures.next().await.unwrap() {
            if !result.is_transaction_error() || futures.is_empty() {
                return result;
            }
        }
    }
    unreachable!("increased gas price past expected limit");
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        contracts::stablex_contract::MockStableXContract,
        gas_price::MockGasPriceEstimating,
        util::{FutureWaitExt as _, MockAsyncSleeping},
    };
    use anyhow::anyhow;
    use assert_approx_eq::assert_approx_eq;
    use futures::future;
    use mockall::predicate::*;
    use std::sync::atomic::{AtomicUsize, Ordering::SeqCst};

    #[test]
    fn infallible_gas_price_estimator_uses_default_and_previous_result() {
        let mut gas_station = MockGasPriceEstimating::new();
        gas_station
            .expect_estimate()
            .times(1)
            .return_once(|| Err(anyhow!("")));
        gas_station
            .expect_estimate()
            .times(1)
            .return_once(|| Ok(5.into()));
        gas_station
            .expect_estimate()
            .times(1)
            .return_once(|| Ok(6.into()));
        gas_station
            .expect_estimate()
            .times(1)
            .return_once(|| Err(anyhow!("")));

        let mut estimator = InfallibleGasPriceEstimator::new(&gas_station, 3.into());
        assert_approx_eq!(estimator.estimate().now_or_never().unwrap(), 3.0);
        assert_approx_eq!(estimator.estimate().now_or_never().unwrap(), 5.0);
        assert_approx_eq!(estimator.estimate().now_or_never().unwrap(), 6.0);
        assert_approx_eq!(estimator.estimate().now_or_never().unwrap(), 6.0);
    }

    #[test]
    fn infallible_gas_price_estimator_does_not_decrease() {
        let mut gas_station = MockGasPriceEstimating::new();
        gas_station
            .expect_estimate()
            .times(1)
            .return_once(|| Ok(10.into()));
        gas_station
            .expect_estimate()
            .times(1)
            .return_once(|| Ok(9.into()));

        let mut estimator = InfallibleGasPriceEstimator::new(&gas_station, 3.into());
        assert_approx_eq!(estimator.estimate().now_or_never().unwrap(), 10.0);
        assert_approx_eq!(estimator.estimate().now_or_never().unwrap(), 10.0);
    }

    #[test]
    fn gas_price_increases_as_expected_and_hits_limit() {
        let estimated = 5.0;
        let cap = 50.0;
        assert_approx_eq!(gas_price(estimated, 0, cap).round(), 5.0);
        assert_approx_eq!(gas_price(estimated, 1, cap).round(), 8.0);
        assert_approx_eq!(gas_price(estimated, 2, cap).round(), 11.0);
        assert_approx_eq!(gas_price(estimated, 3, cap).round(), 17.0);
        assert_approx_eq!(gas_price(estimated, 4, cap).round(), 25.0);
        assert_approx_eq!(gas_price(estimated, 5, cap).round(), 38.0);
        assert_approx_eq!(gas_price(estimated, 6, cap).round(), 50.0);
    }

    #[test]
    fn test_retry_because_timeout() {
        static SUBMIT_SOLUTION_CALL_COUNT: AtomicUsize = AtomicUsize::new(0);

        let mut contract = MockStableXContract::new();
        contract.expect_submit_solution().returning(
            |_, _, _, _, _| match SUBMIT_SOLUTION_CALL_COUNT.fetch_add(1, SeqCst) {
                0 => future::pending().boxed(),
                _ => immediate!(Ok(())),
            },
        );

        let mut sleep = MockAsyncSleeping::new();
        sleep
            .expect_sleep()
            .returning(|_| match SUBMIT_SOLUTION_CALL_COUNT.load(SeqCst) {
                0 | 1 => immediate!(()),
                _ => futures::future::pending().boxed(),
            });

        let mut gas_station = MockGasPriceEstimating::new();
        gas_station.expect_estimate().returning(|| Err(anyhow!("")));

        let args = Args {
            batch_index: 1,
            solution: Solution::trivial(),
            claimed_objective_value: 1.into(),
            gas_price_cap: DEFAULT_GAS_PRICE * 10.0,
            nonce: 0.into(),
        };
        let result = retry(&contract, &gas_station, &sleep, args)
            .now_or_never()
            .unwrap();
        assert!(result.is_ok());
        assert_eq!(SUBMIT_SOLUTION_CALL_COUNT.load(SeqCst), 2);
    }

    #[test]
    fn test_retry_with_gas_price_respects_minimum_increase() {
        let mut contract = MockStableXContract::new();
        contract
            .expect_submit_solution()
            .times(1)
            .with(
                always(),
                always(),
                always(),
                eq(num::f64_to_u256(DEFAULT_GAS_PRICE * 90.0)),
                always(),
            )
            .returning(|_, _, _, _, _| futures::future::pending().boxed());
        // There should not be a second call to submit_solution because 90 to 100 is not a large
        // enough gas price increase.

        let mut gas_station = MockGasPriceEstimating::new();
        gas_station
            .expect_estimate()
            .returning(|| Ok(DEFAULT_GAS_PRICE * 90.0));

        let sleep = MockAsyncSleeping::new();

        let args = Args {
            batch_index: 1,
            solution: Solution::trivial(),
            claimed_objective_value: 1.into(),
            gas_price_cap: DEFAULT_GAS_PRICE * 100.0,
            nonce: 0.into(),
        };
        let result = retry(&contract, &gas_station, &sleep, args).now_or_never();
        assert!(result.is_none());
    }

    #[test]
    fn previous_transaction_completes_first() {
        let mut contract = MockStableXContract::new();
        let mut gas_station = MockGasPriceEstimating::new();
        let mut sleep = MockAsyncSleeping::new();
        let (sender, receiver) = futures::channel::oneshot::channel();

        gas_station.expect_estimate().returning(|| Err(anyhow!("")));
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
        contract
            .expect_submit_solution()
            .times(1)
            .return_once(|_, _, _, _, _| {
                sender.send(()).unwrap();
                futures::future::pending().boxed()
            });
        sleep
            .expect_sleep()
            .returning(|_| future::pending().boxed());

        let args = Args {
            batch_index: 1,
            solution: Solution::trivial(),
            claimed_objective_value: 1.into(),
            gas_price_cap: DEFAULT_GAS_PRICE * 10.0,
            nonce: 0.into(),
        };
        let result = retry(&contract, &gas_station, &sleep, args).wait();
        assert!(result.is_ok());
    }

    fn nonce_error() -> MethodError {
        MethodError {
            signature: String::new(),
            inner: crate::solution_submission::tests::nonce_error(),
        }
    }

    #[test]
    fn nonce_error_ignored() {
        let mut contract = MockStableXContract::new();
        let mut gas_station = MockGasPriceEstimating::new();
        let mut sleep = MockAsyncSleeping::new();
        let (sender, receiver) = futures::channel::oneshot::channel();

        gas_station.expect_estimate().returning(|| Err(anyhow!("")));
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
        contract
            .expect_submit_solution()
            .times(1)
            .return_once(|_, _, _, _, _| {
                sender.send(()).unwrap();
                immediate!(Err(nonce_error()))
            });
        sleep
            .expect_sleep()
            .returning(|_| future::pending().boxed());

        let args = Args {
            batch_index: 1,
            solution: Solution::trivial(),
            claimed_objective_value: 1.into(),
            gas_price_cap: DEFAULT_GAS_PRICE * 10.0,
            nonce: 0.into(),
        };
        let result = retry(&contract, &gas_station, &sleep, args).wait();
        assert!(result.is_ok());
    }
}
