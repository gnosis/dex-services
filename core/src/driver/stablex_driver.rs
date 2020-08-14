use crate::{
    economic_viability::EconomicViabilityComputing,
    metrics::StableXMetrics,
    models::{account_state::AccountState, order::Order, BatchId, Solution},
    orderbook::StableXOrderBookReading,
    price_finding::PriceFinding,
    solution_submission::{SolutionSubmissionError, StableXSolutionSubmitting},
    util::{AsyncSleep, AsyncSleeping},
};
use anyhow::{Error, Result};
use futures::future::{BoxFuture, FutureExt as _};
use log::{info, warn};
use std::{
    sync::Arc,
    time::{Duration, Instant, SystemTime},
};

#[derive(Debug)]
pub enum DriverError {
    Retry(Error),
    Skip(Error),
}

#[cfg_attr(test, mockall::automock)]
pub trait StableXDriver {
    // mockall needs the lifetimes but clippy warns that they are not needed.
    #[allow(clippy::needless_lifetimes)]
    fn run<'a>(
        &'a self,
        batch_to_solve: BatchId,
        latest_solution_submit_time: Duration,
        earliest_solution_submit_time: Duration,
    ) -> BoxFuture<'a, Result<(), DriverError>>;
}

pub struct StableXDriverImpl<'a> {
    price_finder: &'a (dyn PriceFinding + Sync),
    orderbook_reader: &'a (dyn StableXOrderBookReading),
    solution_submitter: &'a (dyn StableXSolutionSubmitting + Sync),
    economic_viability: Arc<dyn EconomicViabilityComputing + Sync>,
    metrics: &'a StableXMetrics,
    sleep: Box<dyn AsyncSleeping>,
}

impl<'a> StableXDriverImpl<'a> {
    pub fn new(
        price_finder: &'a (dyn PriceFinding + Sync),
        orderbook_reader: &'a (dyn StableXOrderBookReading),
        solution_submitter: &'a (dyn StableXSolutionSubmitting + Sync),
        economic_viability: Arc<dyn EconomicViabilityComputing + Sync>,
        metrics: &'a StableXMetrics,
    ) -> Self {
        Self::with_sleep(
            price_finder,
            orderbook_reader,
            solution_submitter,
            economic_viability,
            metrics,
            Box::new(AsyncSleep {}),
        )
    }

    pub fn with_sleep(
        price_finder: &'a (dyn PriceFinding + Sync),
        orderbook_reader: &'a (dyn StableXOrderBookReading),
        solution_submitter: &'a (dyn StableXSolutionSubmitting + Sync),
        economic_viability: Arc<dyn EconomicViabilityComputing + Sync>,
        metrics: &'a StableXMetrics,
        sleep: Box<dyn AsyncSleeping>,
    ) -> Self {
        Self {
            price_finder,
            orderbook_reader,
            solution_submitter,
            metrics,
            economic_viability,
            sleep,
        }
    }

    async fn get_orderbook(&self, batch_to_solve: u32) -> Result<(AccountState, Vec<Order>)> {
        let get_auction_data_result = self.orderbook_reader.get_auction_data(batch_to_solve).await;
        self.metrics
            .auction_orders_fetched(batch_to_solve, &get_auction_data_result);
        get_auction_data_result
    }

    async fn solve(
        &self,
        batch_to_solve: BatchId,
        latest_solution_submit_time: Duration,
        account_state: AccountState,
        orders: Vec<Order>,
    ) -> Result<Solution> {
        if orders.is_empty() {
            info!("No orders in batch {}", batch_to_solve);
            return Ok(Solution::trivial());
        }
        let price_finder_result = self
            .price_finder
            .find_prices(&orders, &account_state, latest_solution_submit_time)
            .await;
        self.metrics
            .auction_solution_computed(batch_to_solve.into(), &price_finder_result);

        let solution = price_finder_result?;
        info!(
            "Computed solution for batch {}: {:?}",
            batch_to_solve, &solution
        );
        Ok(solution)
    }

    async fn submit(
        &self,
        batch_to_solve: BatchId,
        earliest_solution_submit_time: Duration,
        solution: Solution,
    ) -> Result<()> {
        let verified = if solution.is_non_trivial() {
            // NOTE: in retrieving the objective value from the reader the
            //   solution gets validated, ensured that it is better than the
            //   latest submitted solution, and that solutions are still being
            //   accepted for this batch ID.
            let verification_result = self
                .solution_submitter
                .get_solution_objective_value(batch_to_solve.into(), solution.clone())
                .await;
            self.metrics
                .auction_solution_verified(batch_to_solve.into(), &verification_result);

            match verification_result {
                Ok(objective_value) => {
                    info!(
                        "Verified solution with objective value: {}",
                        objective_value
                    );
                    Some(objective_value)
                }
                Err(err) => match err {
                    SolutionSubmissionError::Benign(reason) => {
                        info!("Benign failure while verifying solution: {}", reason);
                        None
                    }
                    SolutionSubmissionError::Unexpected(err) => {
                        // Return from entire function with the unexpected error
                        return Err(err);
                    }
                },
            }
        } else {
            info!(
                "Not submitting trivial solution for batch {}",
                batch_to_solve
            );
            None
        };

        let submitted = if let Some(objective_value) = verified {
            // In the e2e test batch ids don't always correspond to the correct batch id based on
            // real time (we fake advancement of time) so without the outer if we would be waiting
            // a long time. There will likely be a refactor that lets the scheduler decider when
            // solutions should be submitted which fixes this problem in a better way.
            if earliest_solution_submit_time > Duration::from_secs(0) {
                if let Ok(duration) = (batch_to_solve.solve_start_time()
                    + earliest_solution_submit_time)
                    .duration_since(SystemTime::now())
                {
                    log::info!(
                        "Sleeping {} seconds for earliest_solution_submit_time.",
                        duration.as_secs()
                    );
                    self.sleep.sleep(duration).await;
                }
            }
            let gas_price_cap = self
                .economic_viability
                .max_gas_price(solution.economic_viability_info())
                .await?;
            let submission_result = self
                .solution_submitter
                .submit_solution(
                    batch_to_solve.into(),
                    solution,
                    objective_value,
                    gas_price_cap,
                )
                .await;
            self.metrics
                .auction_solution_submitted(batch_to_solve.into(), &submission_result);
            match submission_result {
                Ok(_) => {
                    info!("Successfully applied solution to batch {}", batch_to_solve);
                    true
                }
                Err(err) => match err {
                    SolutionSubmissionError::Benign(reason) => {
                        info!("Benign failure while submitting solution: {}", reason);
                        false
                    }
                    SolutionSubmissionError::Unexpected(err) => return Err(err),
                },
            }
        } else {
            false
        };

        if !submitted {
            self.metrics
                .auction_processed_but_not_submitted(batch_to_solve.into());
        };

        Ok(())
    }
}

impl<'a> StableXDriver for StableXDriverImpl<'a> {
    fn run(
        &self,
        batch_to_solve: BatchId,
        latest_solution_submit_time: Duration,
        earliest_solution_submit_time: Duration,
    ) -> BoxFuture<Result<(), DriverError>> {
        async move {
            let deadline = Instant::now() + latest_solution_submit_time;

            self.metrics
                .auction_processing_started(&Ok(batch_to_solve.into()));
            let (account_state, orders) = self
                .get_orderbook(batch_to_solve.into())
                .await
                .map_err(DriverError::Retry)?;

            // Make sure the solver has at least some minimal time to run to have a chance for a
            // solution. This also fixes an assert where the solver fails if the timelimit gets rounded
            // to 0.
            let latest_solution_submit_time = match deadline.checked_duration_since(Instant::now())
            {
                Some(duration) if duration > Duration::from_secs(1) => duration,
                _ => {
                    warn!("orderbook retrieval exceeded time limit");
                    return Ok(());
                }
            };

            let solution = self
                .solve(
                    batch_to_solve,
                    latest_solution_submit_time,
                    account_state,
                    orders,
                )
                .await
                .map_err(DriverError::Skip)?;
            self.submit(batch_to_solve, earliest_solution_submit_time, solution)
                .await
                .map_err(DriverError::Skip)
        }
        .boxed()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        economic_viability::{FixedEconomicViabilityComputer, MockEconomicViabilityComputing},
        models::{
            order::test_util::{create_order_for_test, order_to_executed_order},
            AccountState,
        },
        orderbook::MockStableXOrderBookReading,
        price_finding::price_finder_interface::MockPriceFinding,
        solution_submission::MockStableXSolutionSubmitting,
        util::{test_util::map_from_slice, MockAsyncSleeping},
    };
    use anyhow::anyhow;
    use ethcontract::U256;
    use mockall::predicate::*;
    use std::thread;

    #[test]
    fn invokes_solver_with_reader_data_for_unprocessed_auction() {
        let mut reader = MockStableXOrderBookReading::default();
        let mut submitter = MockStableXSolutionSubmitting::default();
        let mut pf = MockPriceFinding::default();
        let economic_viability =
            Arc::new(FixedEconomicViabilityComputer::new(None, Some(0.into())));
        let metrics = StableXMetrics::default();

        let orders = vec![create_order_for_test(), create_order_for_test()];
        let state = AccountState::with_balance_for(&orders);

        let batch = 42;
        let latest_solution_submit_time = Duration::from_secs(120);

        reader
            .expect_get_auction_data()
            .with(eq(batch))
            .return_once({
                let result = (state.clone(), orders.clone());
                |_| async { Ok(result) }.boxed()
            });

        submitter
            .expect_get_solution_objective_value()
            .with(eq(batch), always())
            .returning(|_, _| async { Ok(U256::from(1337)) }.boxed());

        submitter
            .expect_submit_solution()
            .with(eq(batch), always(), eq(U256::from(1337)), always())
            .returning(|_, _, _, _| async { Ok(()) }.boxed());

        let solution = Solution {
            prices: map_from_slice(&[(0, 1), (1, 2)]),
            executed_orders: vec![
                order_to_executed_order(&orders[0], 0, 0),
                order_to_executed_order(&orders[1], 2, 2),
            ],
        };
        pf.expect_find_prices()
            .withf(move |o, s, t| {
                o == orders.as_slice() && *s == state && *t <= latest_solution_submit_time
            })
            .return_once(move |_, _, _| async { Ok(solution) }.boxed());

        let driver = StableXDriverImpl::new(&pf, &reader, &submitter, economic_viability, &metrics);
        assert!(driver
            .run(
                BatchId::from(batch),
                latest_solution_submit_time,
                Duration::from_secs(0)
            )
            .now_or_never()
            .unwrap()
            .is_ok());
    }

    #[test]
    fn test_errors_on_failing_reader() {
        let mut reader = MockStableXOrderBookReading::default();
        let submitter = MockStableXSolutionSubmitting::default();
        let pf = MockPriceFinding::default();
        let metrics = StableXMetrics::default();
        let economic_viability = Arc::new(MockEconomicViabilityComputing::new());

        reader
            .expect_get_auction_data()
            .returning(|_| async { Err(anyhow!("Error")) }.boxed());

        let driver = StableXDriverImpl::new(&pf, &reader, &submitter, economic_viability, &metrics);

        assert!(matches!(
            driver
                .run(BatchId(42), Duration::default(), Duration::default())
                .now_or_never()
                .unwrap(),
            Err(DriverError::Retry(_))
        ));
    }

    #[test]
    fn test_errors_on_failing_price_finder() {
        let mut reader = MockStableXOrderBookReading::default();
        let mut submitter = MockStableXSolutionSubmitting::default();
        let mut pf = MockPriceFinding::default();
        let economic_viability =
            Arc::new(FixedEconomicViabilityComputer::new(None, Some(0.into())));
        let metrics = StableXMetrics::default();

        let orders = vec![create_order_for_test(), create_order_for_test()];
        let state = AccountState::with_balance_for(&orders);

        let batch = 42;
        let latest_solution_submit_time = Duration::from_secs(120);

        reader
            .expect_get_auction_data()
            .with(eq(batch))
            .return_once({
                let result = (state, orders);
                |_| async { Ok(result) }.boxed()
            });

        submitter
            .expect_get_solution_objective_value()
            .with(eq(batch), always())
            .returning(|_, _| async { Ok(U256::from(1337)) }.boxed());

        submitter
            .expect_submit_solution()
            .with(eq(batch), always(), eq(U256::from(1337)), always())
            .returning(|_, _, _, _| async { Ok(()) }.boxed());

        pf.expect_find_prices()
            .returning(|_, _, _| async { Err(anyhow!("Error")) }.boxed());

        let driver = StableXDriverImpl::new(&pf, &reader, &submitter, economic_viability, &metrics);

        assert!(matches!(
            driver
                .run(
                    BatchId::from(batch),
                    latest_solution_submit_time,
                    Duration::from_secs(0)
                )
                .now_or_never()
                .unwrap(),
            Err(DriverError::Skip(_))
        ));
    }

    #[test]
    fn test_do_not_invoke_solver_when_no_orders() {
        let mut reader = MockStableXOrderBookReading::default();
        let submitter = MockStableXSolutionSubmitting::default();
        let pf = MockPriceFinding::default();
        let economic_viability = Arc::new(MockEconomicViabilityComputing::new());
        let metrics = StableXMetrics::default();

        let orders = vec![];
        let state = AccountState::with_balance_for(&orders);

        let batch = 42;
        let latest_solution_submit_time = Duration::from_secs(120);

        reader
            .expect_get_auction_data()
            .with(eq(batch))
            .return_once(move |_| async { Ok((state, orders)) }.boxed());

        let driver = StableXDriverImpl::new(&pf, &reader, &submitter, economic_viability, &metrics);
        assert!(driver
            .run(
                BatchId::from(batch),
                latest_solution_submit_time,
                Duration::from_secs(0)
            )
            .now_or_never()
            .unwrap()
            .is_ok());
    }

    #[test]
    fn test_does_not_submit_empty_solution() {
        let mut reader = MockStableXOrderBookReading::default();
        let mut submitter = MockStableXSolutionSubmitting::default();
        let mut pf = MockPriceFinding::default();
        let economic_viability = Arc::new(MockEconomicViabilityComputing::new());
        let metrics = StableXMetrics::default();

        let orders = vec![create_order_for_test(), create_order_for_test()];
        let state = AccountState::with_balance_for(&orders);

        let batch = 42;
        let latest_solution_submit_time = Duration::from_secs(120);

        reader
            .expect_get_auction_data()
            .with(eq(batch))
            .return_once({
                let result = (state.clone(), orders.clone());
                |_| async { Ok(result) }.boxed()
            });

        let solution = Solution::trivial();
        pf.expect_find_prices()
            .withf(move |o, s, t| {
                o == orders.as_slice() && *s == state && *t <= latest_solution_submit_time
            })
            .return_once(move |_, _, _| async { Ok(solution) }.boxed());

        submitter.expect_submit_solution().times(0);

        let driver = StableXDriverImpl::new(&pf, &reader, &submitter, economic_viability, &metrics);
        assert!(driver
            .run(
                BatchId::from(batch),
                latest_solution_submit_time,
                Duration::from_secs(0)
            )
            .now_or_never()
            .unwrap()
            .is_ok());
    }

    #[test]
    fn test_does_not_submit_solution_for_which_validation_failed() {
        let mut reader = MockStableXOrderBookReading::default();
        let mut submitter = MockStableXSolutionSubmitting::default();
        let mut pf = MockPriceFinding::default();
        let economic_viability = Arc::new(MockEconomicViabilityComputing::new());
        let metrics = StableXMetrics::default();

        let orders = vec![create_order_for_test(), create_order_for_test()];
        let state = AccountState::with_balance_for(&orders);

        let batch = 42;
        let latest_solution_submit_time = Duration::from_secs(120);

        reader
            .expect_get_auction_data()
            .with(eq(batch))
            .return_once({
                let result = (state.clone(), orders.clone());
                |_| async { Ok(result) }.boxed()
            });

        submitter
            .expect_get_solution_objective_value()
            .with(eq(batch), always())
            .returning(|_, _| {
                async {
                    Err(SolutionSubmissionError::Unexpected(anyhow!(
                        "get_solution_objective_value failed"
                    )))
                }
                .boxed()
            });
        submitter.expect_submit_solution().times(0);

        let solution = Solution {
            prices: map_from_slice(&[(0, 1), (1, 2)]),
            executed_orders: vec![
                order_to_executed_order(&orders[0], 0, 0),
                order_to_executed_order(&orders[1], 2, 2),
            ],
        };
        pf.expect_find_prices()
            .withf(move |o, s, t| {
                o == orders.as_slice() && *s == state && *t <= latest_solution_submit_time
            })
            .return_once(move |_, _, _| async { Ok(solution) }.boxed());

        let driver = StableXDriverImpl::new(&pf, &reader, &submitter, economic_viability, &metrics);
        assert!(matches!(
            driver
                .run(
                    BatchId::from(batch),
                    latest_solution_submit_time,
                    Duration::from_secs(0)
                )
                .now_or_never()
                .unwrap(),
            Err(DriverError::Skip(_))
        ));
    }

    #[test]
    fn test_do_not_fail_on_benign_verification_error() {
        let mut reader = MockStableXOrderBookReading::default();
        let mut submitter = MockStableXSolutionSubmitting::default();
        let mut pf = MockPriceFinding::default();
        let economic_viability = Arc::new(MockEconomicViabilityComputing::new());
        let metrics = StableXMetrics::default();

        let orders = vec![create_order_for_test(), create_order_for_test()];
        let state = AccountState::with_balance_for(&orders);

        let batch = 42;
        let latest_solution_submit_time = Duration::from_secs(120);

        reader
            .expect_get_auction_data()
            .with(eq(batch))
            .return_once({
                let result = (state.clone(), orders.clone());
                |_| async { Ok(result) }.boxed()
            });

        submitter
            .expect_get_solution_objective_value()
            .with(eq(batch), always())
            .returning(|_, _| {
                async { Err(SolutionSubmissionError::Benign("Benign Error".to_owned())) }.boxed()
            });
        submitter.expect_submit_solution().times(0);

        let solution = Solution {
            prices: map_from_slice(&[(0, 1), (1, 2)]),
            executed_orders: vec![
                order_to_executed_order(&orders[0], 0, 0),
                order_to_executed_order(&orders[1], 2, 2),
            ],
        };
        pf.expect_find_prices()
            .withf(move |o, s, t| {
                o == orders.as_slice() && *s == state && *t <= latest_solution_submit_time
            })
            .return_once(move |_, _, _| async { Ok(solution) }.boxed());

        let driver = StableXDriverImpl::new(&pf, &reader, &submitter, economic_viability, &metrics);
        assert!(driver
            .run(
                BatchId::from(batch),
                latest_solution_submit_time,
                Duration::from_secs(0)
            )
            .now_or_never()
            .unwrap()
            .is_ok());
    }

    #[test]
    fn test_do_not_fail_on_benign_submission_error() {
        let mut reader = MockStableXOrderBookReading::default();
        let mut submitter = MockStableXSolutionSubmitting::default();
        let mut pf = MockPriceFinding::default();
        let economic_viability =
            Arc::new(FixedEconomicViabilityComputer::new(None, Some(0.into())));
        let metrics = StableXMetrics::default();

        let orders = vec![create_order_for_test(), create_order_for_test()];
        let state = AccountState::with_balance_for(&orders);

        let batch = 42;
        let latest_solution_submit_time = Duration::from_secs(120);

        reader
            .expect_get_auction_data()
            .with(eq(batch))
            .return_once({
                let result = (state.clone(), orders.clone());
                |_| async { Ok(result) }.boxed()
            });

        submitter
            .expect_get_solution_objective_value()
            .with(eq(batch), always())
            .returning(|_, _| async { Ok(42.into()) }.boxed());
        submitter
            .expect_submit_solution()
            .with(eq(batch), always(), eq(U256::from(42)), always())
            .returning(|_, _, _, _| {
                async { Err(SolutionSubmissionError::Benign("Benign Error".to_owned())) }.boxed()
            });

        let solution = Solution {
            prices: map_from_slice(&[(0, 1), (1, 2)]),
            executed_orders: vec![
                order_to_executed_order(&orders[0], 0, 0),
                order_to_executed_order(&orders[1], 2, 2),
            ],
        };
        pf.expect_find_prices()
            .withf(move |o, s, t| {
                o == orders.as_slice() && *s == state && *t <= latest_solution_submit_time
            })
            .return_once(move |_, _, _| async { Ok(solution) }.boxed());

        let driver = StableXDriverImpl::new(&pf, &reader, &submitter, economic_viability, &metrics);
        assert!(driver
            .run(
                BatchId::from(batch),
                latest_solution_submit_time,
                Duration::from_secs(0)
            )
            .now_or_never()
            .unwrap()
            .is_ok());
    }

    #[test]
    fn does_not_invoke_price_finder_when_orderbook_retrieval_exceedes_latest_solution_submit_time()
    {
        let mut reader = MockStableXOrderBookReading::default();
        let submitter = MockStableXSolutionSubmitting::default();
        let pf = MockPriceFinding::default();
        let economic_viability = Arc::new(MockEconomicViabilityComputing::new());
        let metrics = StableXMetrics::default();

        let orders = vec![create_order_for_test(), create_order_for_test()];
        let state = AccountState::with_balance_for(&orders);

        let batch = 42;
        let latest_solution_submit_time = Duration::from_secs(0);

        reader
            .expect_get_auction_data()
            .with(eq(batch))
            .return_once(move |_| {
                // NOTE: Wait for an epsilon to go by so that the time limit
                //   is exceeded.
                let start = Instant::now();
                while start.elapsed() <= latest_solution_submit_time {
                    thread::yield_now();
                }
                async { Ok((state, orders)) }.boxed()
            });

        let driver = StableXDriverImpl::new(&pf, &reader, &submitter, economic_viability, &metrics);
        assert!(driver
            .run(
                BatchId::from(batch),
                latest_solution_submit_time,
                Duration::from_secs(0)
            )
            .now_or_never()
            .unwrap()
            .is_ok());
    }

    #[test]
    fn waits_for_solution_submit_time() {
        let mut reader = MockStableXOrderBookReading::default();
        let mut submitter = MockStableXSolutionSubmitting::default();
        let mut pf = MockPriceFinding::default();
        let economic_viability =
            Arc::new(FixedEconomicViabilityComputer::new(None, Some(0.into())));
        let metrics = StableXMetrics::default();
        let mut sleep = MockAsyncSleeping::new();

        let orders = vec![create_order_for_test(), create_order_for_test()];
        let state = AccountState::with_balance_for(&orders);
        let solution = Solution {
            prices: map_from_slice(&[(0, 1), (1, 2)]),
            executed_orders: vec![
                order_to_executed_order(&orders[0], 0, 0),
                order_to_executed_order(&orders[1], 2, 2),
            ],
        };

        reader.expect_get_auction_data().return_once({
            let result = (state, orders);
            |_| immediate!(Ok(result))
        });
        submitter
            .expect_get_solution_objective_value()
            .returning(|_, _| immediate!(Ok(U256::from(1337))));
        submitter
            .expect_submit_solution()
            .returning(|_, _, _, _| immediate!(Ok(())));
        pf.expect_find_prices()
            .return_once(move |_, _, _| immediate!(Ok(solution)));
        sleep.expect_sleep().times(1).returning(|_| immediate!(()));

        let driver = StableXDriverImpl::with_sleep(
            &pf,
            &reader,
            &submitter,
            economic_viability,
            &metrics,
            Box::new(sleep),
        );
        assert!(driver
            .run(
                BatchId::now(),
                Duration::from_secs(100),
                Duration::from_secs(200)
            )
            .now_or_never()
            .unwrap()
            .is_ok());
    }
}
