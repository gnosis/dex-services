use crate::metrics::StableXMetrics;
use crate::models::{account_state::AccountState, order::Order, Solution};
use crate::orderbook::StableXOrderBookReading;
use crate::price_finding::PriceFinding;
use crate::solution_submission::{SolutionSubmissionError, StableXSolutionSubmitting};
use crate::util::FutureWaitExt as _;
use anyhow::{Error, Result};
use ethcontract::U256;
use log::{info, warn};
use std::time::{Duration, Instant};

#[derive(Debug)]
pub enum DriverResult {
    Ok,
    Retry(Error),
    Skip(Error),
}

#[cfg_attr(test, mockall::automock)]
pub trait StableXDriver {
    fn run(&self, batch_to_solve: U256, time_limit: Duration) -> DriverResult;
}

pub struct StableXDriverImpl<'a> {
    price_finder: &'a (dyn PriceFinding + Sync),
    orderbook_reader: &'a (dyn StableXOrderBookReading),
    solution_submitter: &'a (dyn StableXSolutionSubmitting + Sync),
    metrics: &'a StableXMetrics,
}

impl<'a> StableXDriverImpl<'a> {
    pub fn new(
        price_finder: &'a (dyn PriceFinding + Sync),
        orderbook_reader: &'a (dyn StableXOrderBookReading),
        solution_submitter: &'a (dyn StableXSolutionSubmitting + Sync),
        metrics: &'a StableXMetrics,
    ) -> Self {
        Self {
            price_finder,
            orderbook_reader,
            solution_submitter,
            metrics,
        }
    }

    fn get_orderbook(&self, batch_to_solve: U256) -> Result<(AccountState, Vec<Order>)> {
        let get_auction_data_result = self
            .orderbook_reader
            .get_auction_data(batch_to_solve)
            .wait();
        self.metrics
            .auction_orders_fetched(batch_to_solve, &get_auction_data_result);
        get_auction_data_result
    }

    fn solve(
        &self,
        batch_to_solve: U256,
        time_limit: Duration,
        account_state: AccountState,
        orders: Vec<Order>,
    ) -> Result<()> {
        let solution = if orders.is_empty() {
            info!("No orders in batch {}", batch_to_solve);
            Solution::trivial()
        } else {
            let price_finder_result = self
                .price_finder
                .find_prices(&orders, &account_state, time_limit)
                .wait();
            self.metrics
                .auction_solution_computed(batch_to_solve, &price_finder_result);

            let solution = price_finder_result?;
            info!(
                "Computed solution for batch {}: {:?}",
                batch_to_solve, &solution
            );

            solution
        };

        let verified = if solution.is_non_trivial() {
            // NOTE: in retrieving the objective value from the reader the
            //   solution gets validated, ensured that it is better than the
            //   latest submitted solution, and that solutions are still being
            //   accepted for this batch ID.
            let verification_result = self
                .solution_submitter
                .get_solution_objective_value(batch_to_solve, solution.clone())
                .wait();
            self.metrics
                .auction_solution_verified(batch_to_solve, &verification_result);

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
            let submission_result = self
                .solution_submitter
                .submit_solution(batch_to_solve, solution, objective_value)
                .wait();
            self.metrics
                .auction_solution_submitted(batch_to_solve, &submission_result);
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
                .auction_processed_but_not_submitted(batch_to_solve);
        };

        Ok(())
    }
}

impl<'a> StableXDriver for StableXDriverImpl<'a> {
    fn run(&self, batch_to_solve: U256, time_limit: Duration) -> DriverResult {
        let deadline = Instant::now() + time_limit;

        self.metrics.auction_processing_started(&Ok(batch_to_solve));
        let (account_state, orders) = match self.get_orderbook(batch_to_solve) {
            Ok(ok) => ok,
            Err(err) => return DriverResult::Retry(err),
        };

        // Make sure the solver has at least some minimal time to run to have a chance for a
        // solution. This also fixes an assert where the solver fails if the timelimit gets rounded
        // to 0.
        let price_finding_time_limit = match deadline.checked_duration_since(Instant::now()) {
            Some(time_limit) if time_limit > Duration::from_secs(1) => time_limit,
            _ => {
                warn!("orderbook retrieval exceeded time limit");
                return DriverResult::Ok;
            }
        };

        match self.solve(
            batch_to_solve,
            price_finding_time_limit,
            account_state,
            orders,
        ) {
            Ok(()) => DriverResult::Ok,
            Err(err) => DriverResult::Skip(err),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::order::test_util::{create_order_for_test, order_to_executed_order};
    use crate::models::AccountState;
    use crate::orderbook::MockStableXOrderBookReading;
    use crate::price_finding::price_finder_interface::MockPriceFinding;
    use crate::solution_submission::MockStableXSolutionSubmitting;
    use crate::util::test_util::map_from_slice;
    use anyhow::anyhow;
    use futures::future::FutureExt as _;
    use mockall::predicate::*;
    use std::thread;

    impl DriverResult {
        fn is_ok(&self) -> bool {
            match self {
                DriverResult::Ok => true,
                _ => false,
            }
        }

        fn is_retry(&self) -> bool {
            match self {
                DriverResult::Retry(_) => true,
                _ => false,
            }
        }

        fn is_skip(&self) -> bool {
            match self {
                DriverResult::Skip(_) => true,
                _ => false,
            }
        }
    }

    #[test]
    fn invokes_solver_with_reader_data_for_unprocessed_auction() {
        let mut reader = MockStableXOrderBookReading::default();
        let mut submitter = MockStableXSolutionSubmitting::default();
        let mut pf = MockPriceFinding::default();
        let metrics = StableXMetrics::default();

        let orders = vec![create_order_for_test(), create_order_for_test()];
        let state = AccountState::with_balance_for(&orders);

        let batch = U256::from(42);
        let time_limit = Duration::from_secs(120);

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
            .with(eq(batch), always(), eq(U256::from(1337)))
            .returning(|_, _, _| async { Ok(()) }.boxed());

        let solution = Solution {
            prices: map_from_slice(&[(0, 1), (1, 2)]),
            executed_orders: vec![
                order_to_executed_order(&orders[0], 0, 0),
                order_to_executed_order(&orders[1], 2, 2),
            ],
        };
        pf.expect_find_prices()
            .withf(move |o, s, t| o == orders.as_slice() && *s == state && *t <= time_limit)
            .return_once(move |_, _, _| async { Ok(solution) }.boxed());

        let driver = StableXDriverImpl::new(&pf, &reader, &submitter, &metrics);
        assert!(driver.run(batch, time_limit).is_ok());
    }

    #[test]
    fn test_errors_on_failing_reader() {
        let mut reader = MockStableXOrderBookReading::default();
        let submitter = MockStableXSolutionSubmitting::default();
        let pf = MockPriceFinding::default();
        let metrics = StableXMetrics::default();

        reader
            .expect_get_auction_data()
            .returning(|_| async { Err(anyhow!("Error")) }.boxed());

        let driver = StableXDriverImpl::new(&pf, &reader, &submitter, &metrics);

        assert!(driver.run(U256::from(42), Duration::default()).is_retry())
    }

    #[test]
    fn test_errors_on_failing_price_finder() {
        let mut reader = MockStableXOrderBookReading::default();
        let mut submitter = MockStableXSolutionSubmitting::default();
        let mut pf = MockPriceFinding::default();
        let metrics = StableXMetrics::default();

        let orders = vec![create_order_for_test(), create_order_for_test()];
        let state = AccountState::with_balance_for(&orders);

        let batch = U256::from(42);
        let time_limit = Duration::from_secs(120);

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
            .with(eq(batch), always(), eq(U256::from(1337)))
            .returning(|_, _, _| async { Ok(()) }.boxed());

        pf.expect_find_prices()
            .returning(|_, _, _| async { Err(anyhow!("Error")) }.boxed());

        let driver = StableXDriverImpl::new(&pf, &reader, &submitter, &metrics);

        assert!(driver.run(batch, time_limit).is_skip());
    }

    #[test]
    fn test_do_not_invoke_solver_when_no_orders() {
        let mut reader = MockStableXOrderBookReading::default();
        let submitter = MockStableXSolutionSubmitting::default();
        let pf = MockPriceFinding::default();
        let metrics = StableXMetrics::default();

        let orders = vec![];
        let state = AccountState::with_balance_for(&orders);

        let batch = U256::from(42);
        let time_limit = Duration::from_secs(120);

        reader
            .expect_get_auction_data()
            .with(eq(batch))
            .return_once(move |_| async { Ok((state, orders)) }.boxed());

        let driver = StableXDriverImpl::new(&pf, &reader, &submitter, &metrics);
        assert!(driver.run(batch, time_limit).is_ok());
    }

    #[test]
    fn test_does_not_submit_empty_solution() {
        let mut reader = MockStableXOrderBookReading::default();
        let mut submitter = MockStableXSolutionSubmitting::default();
        let mut pf = MockPriceFinding::default();
        let metrics = StableXMetrics::default();

        let orders = vec![create_order_for_test(), create_order_for_test()];
        let state = AccountState::with_balance_for(&orders);

        let batch = U256::from(42);
        let time_limit = Duration::from_secs(120);

        reader
            .expect_get_auction_data()
            .with(eq(batch))
            .return_once({
                let result = (state.clone(), orders.clone());
                |_| async { Ok(result) }.boxed()
            });

        let solution = Solution::trivial();
        pf.expect_find_prices()
            .withf(move |o, s, t| o == orders.as_slice() && *s == state && *t <= time_limit)
            .return_once(move |_, _, _| async { Ok(solution) }.boxed());

        submitter.expect_submit_solution().times(0);

        let driver = StableXDriverImpl::new(&pf, &reader, &submitter, &metrics);
        assert!(driver.run(batch, time_limit).is_ok());
    }

    #[test]
    fn test_does_not_submit_solution_for_which_validation_failed() {
        let mut reader = MockStableXOrderBookReading::default();
        let mut submitter = MockStableXSolutionSubmitting::default();
        let mut pf = MockPriceFinding::default();
        let metrics = StableXMetrics::default();

        let orders = vec![create_order_for_test(), create_order_for_test()];
        let state = AccountState::with_balance_for(&orders);

        let batch = U256::from(42);
        let time_limit = Duration::from_secs(120);

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
            .withf(move |o, s, t| o == orders.as_slice() && *s == state && *t <= time_limit)
            .return_once(move |_, _, _| async { Ok(solution) }.boxed());

        let driver = StableXDriverImpl::new(&pf, &reader, &submitter, &metrics);
        assert!(driver.run(batch, time_limit).is_skip());
    }

    #[test]
    fn test_do_not_fail_on_benign_verification_error() {
        let mut reader = MockStableXOrderBookReading::default();
        let mut submitter = MockStableXSolutionSubmitting::default();
        let mut pf = MockPriceFinding::default();
        let metrics = StableXMetrics::default();

        let orders = vec![create_order_for_test(), create_order_for_test()];
        let state = AccountState::with_balance_for(&orders);

        let batch = U256::from(42);
        let time_limit = Duration::from_secs(120);

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
            .withf(move |o, s, t| o == orders.as_slice() && *s == state && *t <= time_limit)
            .return_once(move |_, _, _| async { Ok(solution) }.boxed());

        let driver = StableXDriverImpl::new(&pf, &reader, &submitter, &metrics);
        assert!(driver.run(batch, time_limit).is_ok());
    }

    #[test]
    fn test_do_not_fail_on_benign_submission_error() {
        let mut reader = MockStableXOrderBookReading::default();
        let mut submitter = MockStableXSolutionSubmitting::default();
        let mut pf = MockPriceFinding::default();
        let metrics = StableXMetrics::default();

        let orders = vec![create_order_for_test(), create_order_for_test()];
        let state = AccountState::with_balance_for(&orders);

        let batch = U256::from(42);
        let time_limit = Duration::from_secs(120);

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
            .with(eq(batch), always(), eq(U256::from(42)))
            .returning(|_, _, _| {
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
            .withf(move |o, s, t| o == orders.as_slice() && *s == state && *t <= time_limit)
            .return_once(move |_, _, _| async { Ok(solution) }.boxed());

        let driver = StableXDriverImpl::new(&pf, &reader, &submitter, &metrics);
        assert!(driver.run(batch, time_limit).is_ok());
    }

    #[test]
    fn does_not_invoke_price_finder_when_orderbook_retrieval_exceedes_time_limit() {
        let mut reader = MockStableXOrderBookReading::default();
        let submitter = MockStableXSolutionSubmitting::default();
        let pf = MockPriceFinding::default();
        let metrics = StableXMetrics::default();

        let orders = vec![create_order_for_test(), create_order_for_test()];
        let state = AccountState::with_balance_for(&orders);

        let batch = U256::from(42);
        let time_limit = Duration::from_secs(0);

        reader
            .expect_get_auction_data()
            .with(eq(batch))
            .return_once(move |_| {
                // NOTE: Wait for an epsilon to go by so that the time limit
                //   is exceeded.
                let start = Instant::now();
                while start.elapsed() <= time_limit {
                    thread::yield_now();
                }
                async { Ok((state, orders)) }.boxed()
            });

        let driver = StableXDriverImpl::new(&pf, &reader, &submitter, &metrics);
        assert!(driver.run(batch, time_limit).is_ok());
    }
}
