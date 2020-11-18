use super::IsOpenEthereumTransactionError as _;
use crate::util::{self, AsyncSleeping, Now};
use crate::{
    contracts::stablex_contract::{StableXContract, SOLUTION_SUBMISSION_GAS_LIMIT},
    models::Solution,
};
use anyhow::Result;
use ethcontract::errors::MethodError;
use futures::{
    future::{BoxFuture, FutureExt as _},
    stream::{futures_unordered::FuturesUnordered, StreamExt as _},
};
use gas_estimation::GasPriceEstimating;
use primitive_types::U256;
use std::{
    sync::Arc,
    time::{Duration, Instant},
};

use super::MIN_GAS_PRICE_INCREASE_FACTOR;

const GAS_PRICE_REFRESH_INTERVAL: Duration = Duration::from_secs(15);

pub struct Args {
    pub batch_index: u32,
    pub solution: Solution,
    pub claimed_objective_value: U256,
    pub gas_price_cap: f64,
    pub nonce: U256,
    pub target_confirm_time: Instant,
}

#[cfg_attr(test, mockall::automock)]
pub trait SolutionTransactionSending {
    /// Submit the solution with an appropriate gas price based on target_confirm_time. Until the
    /// transaction has been confirmed the gas price is continually updated.
    fn retry<'a>(&'a self, args: Args) -> BoxFuture<'a, Result<(), MethodError>>;
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
    fn retry<'b>(&'b self, args: Args) -> BoxFuture<'b, Result<(), MethodError>> {
        self.retry_(args).boxed()
    }
}

#[derive(Debug)]
enum FutureOutput {
    SolutionSubmission(Result<(), MethodError>),
    Timeout,
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
    async fn gas_price(&self, args: &Args) -> Result<f64> {
        let time_remaining = args
            .target_confirm_time
            .saturating_duration_since(self.now.instant_now());
        // TODO: Use a more accurate gas limit once the gas estimators take that into account.
        self.gas_price_estimating
            .estimate_with_limits(SOLUTION_SUBMISSION_GAS_LIMIT as f64, time_remaining)
            .await
    }

    fn submit_solution(&self, args: &Args, gas_price: f64) -> BoxFuture<FutureOutput> {
        self.contract
            .submit_solution(
                args.batch_index,
                args.solution.clone(),
                args.claimed_objective_value,
                U256::from_f64_lossy(gas_price),
                args.nonce,
            )
            .map(FutureOutput::SolutionSubmission)
            .boxed()
    }

    fn wait_interval(&self) -> BoxFuture<FutureOutput> {
        self.async_sleep
            .sleep(GAS_PRICE_REFRESH_INTERVAL)
            .map(|()| FutureOutput::Timeout)
            .boxed()
    }

    async fn retry_(&self, args: Args) -> Result<(), MethodError> {
        // Like in `StableXSolutionSubmitter::submit_solution` we need to handle the situation where
        // we observe a nonce error from one future before the completion of another. It is also
        // possible that a previous submission transaction completes first instead of the most
        // recent one.
        // That is why we keep track of all past transaction futures in this variable.
        let mut futures = FuturesUnordered::new();
        let mut last_used_gas_price = 0.0;
        log::debug!(
            "starting transaction retry with gas price cap {}",
            args.gas_price_cap
        );
        loop {
            match self.gas_price(&args).await {
                Ok(gas_price) => {
                    log::debug!("estimated gas price {}", gas_price);
                    if let Some(mut gas_price) =
                        new_gas_price_estimate(last_used_gas_price, gas_price, args.gas_price_cap)
                    {
                        gas_price = gas_price.ceil();
                        last_used_gas_price = gas_price;
                        log::info!("submitting solution transaction at gas price {}", gas_price);
                        futures.push(self.submit_solution(&args, gas_price));
                    }
                }
                Err(err) => log::error!("gas estimation failed: {:?}", err),
            }

            // We also store the sleep future and have converted both future's return type to the
            // `FutureOutput` enum so that they can be stored in the FuturesUnordered.
            futures.push(self.wait_interval());

            // We check which type of future completes first. We can unwrap `next` because there is
            // always the wait_interval future. If it completes first then `while let` does not
            // match and we go into the next loop with a new gas price.
            // We need a while loop to work around the possibility of observing transaction futures
            // completing in an unexpected ordering. If we get a transaction error a different
            // transaction future could still complete.
            // Only when we observe an error that isn't a transaction error or we get to the last
            // transaction future we can be sure that we are done.
            while let FutureOutput::SolutionSubmission(result) = futures.next().await.unwrap() {
                // Compare to `1` instead of `empty` because of the wait_interval future.
                let is_last_transaction_future = futures.len() == 1;
                if !result.is_transaction_error() || is_last_transaction_future {
                    return result;
                }
            }
        }
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
    use futures::future;

    #[test]
    fn new_as_price_estimate_() {
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
    fn test_retry_with_gas_price_respects_minimum_increase() {
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
        let result = retry.retry(args).wait();
        assert!(result.is_ok());
    }

    #[test]
    fn previous_transaction_completes_first() {
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
                futures::future::pending().boxed()
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
        let result = retry.retry(args).wait();
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
                immediate!(Err(nonce_error()))
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
        let result = retry.retry(args).wait();
        assert!(result.is_ok());
    }
}
