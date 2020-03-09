use crate::metrics::StableXMetrics;
use crate::models::Solution;
use crate::orderbook::StableXOrderBookReading;
use crate::price_finding::PriceFinding;
use crate::solution_submission::{SolutionSubmissionError, StableXSolutionSubmitting};
use anyhow::Result;
use ethcontract::U256;
use log::info;
use std::collections::HashSet;
use std::thread;
use std::time::{Duration, SystemTime, SystemTimeError};

const BATCH_DURATION: Duration = Duration::from_secs(300);

struct BatchId(pub u64);

impl BatchId {
    pub fn current(now: SystemTime) -> std::result::Result<Self, SystemTimeError> {
        let time_since_epoch = now.duration_since(SystemTime::UNIX_EPOCH)?;
        Ok(Self(time_since_epoch.as_secs() / BATCH_DURATION.as_secs()))
    }

    pub fn start_time(&self) -> SystemTime {
        SystemTime::UNIX_EPOCH + Duration::from_secs(self.0 * BATCH_DURATION.as_secs())
    }
}

pub struct StableXDriver<'a> {
    past_auctions: HashSet<U256>,
    price_finder: &'a mut dyn PriceFinding,
    orderbook_reader: &'a dyn StableXOrderBookReading,
    solution_submitter: &'a dyn StableXSolutionSubmitting,
    metrics: StableXMetrics,
    batch_wait_time: Duration,
    max_batch_elapsed_time: Duration,
}

impl<'a> StableXDriver<'a> {
    pub fn new(
        price_finder: &'a mut dyn PriceFinding,
        orderbook_reader: &'a dyn StableXOrderBookReading,
        solution_submitter: &'a dyn StableXSolutionSubmitting,
        metrics: StableXMetrics,
        batch_wait_time: Duration,
        max_batch_elapsed_time: Duration,
    ) -> Self {
        StableXDriver {
            past_auctions: HashSet::new(),
            price_finder,
            orderbook_reader,
            solution_submitter,
            metrics,
            batch_wait_time,
            max_batch_elapsed_time,
        }
    }

    pub fn run(&mut self) -> Result<bool> {
        self.run_internal(SystemTime::now())
    }

    fn run_internal(&mut self, now: SystemTime) -> Result<bool> {
        let open_batch = BatchId::current(now)?;
        let solving_batch = BatchId(open_batch.0 - 1);
        let batch_to_solve: U256 = solving_batch.0.into();
        let elapsed_time = now.duration_since(solving_batch.start_time())? - BATCH_DURATION;

        info!(
            "Handling batch id {} with elapsed time {}",
            solving_batch.0,
            elapsed_time.as_secs_f64()
        );

        if self.past_auctions.contains(&batch_to_solve) {
            info!("Skipping batch because it has already been handled");
            return Ok(false);
        }

        if elapsed_time > self.max_batch_elapsed_time {
            info!("Skipping batch because there is not enough time left");
            return Ok(false);
        }

        if elapsed_time < self.batch_wait_time {
            thread::sleep(self.batch_wait_time - elapsed_time);
            info!("Starting batch at intended time");
        } else {
            info!("Starting batch later than intended");
        }

        self.metrics.auction_processing_started(&Ok(batch_to_solve));

        let get_auction_data_result = self.orderbook_reader.get_auction_data(batch_to_solve);
        self.metrics
            .auction_orders_fetched(batch_to_solve, &get_auction_data_result);
        let (account_state, orders) = get_auction_data_result?;

        self.past_auctions.insert(batch_to_solve);

        let solution = if orders.is_empty() {
            info!("No orders in batch {}", batch_to_solve);
            Solution::trivial()
        } else {
            let price_finder_result = self.price_finder.find_prices(&orders, &account_state);
            self.metrics
                .auction_solution_computed(batch_to_solve, &price_finder_result);

            let solution = price_finder_result?;
            info!("Computed solution: {:?}", &solution);

            solution
        };

        let verified = if solution.is_non_trivial() {
            // NOTE: in retrieving the objective value from the reader the
            //   solution gets validated, ensured that it is better than the
            //   latest submitted solution, and that solutions are still being
            //   accepted for this batch ID.
            let verification_result = self
                .solution_submitter
                .get_solution_objective_value(batch_to_solve, solution.clone());
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
            let submission_result =
                self.solution_submitter
                    .submit_solution(batch_to_solve, solution, objective_value);
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
            self.metrics.auction_skipped(batch_to_solve);
        };
        Ok(submitted)
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
    use mockall::predicate::*;

    fn test_driver<'a>(
        price_finder: &'a mut dyn PriceFinding,
        orderbook_reader: &'a dyn StableXOrderBookReading,
        solution_submitter: &'a dyn StableXSolutionSubmitting,
        metrics: StableXMetrics,
    ) -> StableXDriver<'a> {
        StableXDriver::new(
            price_finder,
            orderbook_reader,
            solution_submitter,
            metrics,
            Duration::from_secs(0),
            Duration::from_secs(std::u64::MAX),
        )
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

        reader
            .expect_get_auction_data()
            .with(eq(batch))
            .times(1)
            .return_once({
                let result = (state.clone(), orders.clone());
                |_| Ok(result)
            });

        submitter
            .expect_get_solution_objective_value()
            .with(eq(batch), always())
            .times(1)
            .returning(|_, _| Ok(U256::from(1337)));

        submitter
            .expect_submit_solution()
            .with(eq(batch), always(), eq(U256::from(1337)))
            .times(1)
            .returning(|_, _, _| Ok(()));

        let solution = Solution {
            prices: map_from_slice(&[(0, 1), (1, 2)]),
            executed_orders: vec![
                order_to_executed_order(&orders[0], 0, 0),
                order_to_executed_order(&orders[1], 2, 2),
            ],
        };
        pf.expect_find_prices()
            .withf(move |o, s| o == orders.as_slice() && *s == state)
            .return_once(move |_, _| Ok(solution));

        let mut driver = test_driver(&mut pf, &reader, &submitter, metrics);
        assert!(driver
            .run_internal(SystemTime::UNIX_EPOCH + BATCH_DURATION * 43)
            .unwrap());
    }

    #[test]
    fn does_not_process_previously_processed_auction_again() {
        let mut reader = MockStableXOrderBookReading::default();
        let mut submitter = MockStableXSolutionSubmitting::default();
        let mut pf = MockPriceFinding::default();
        let metrics = StableXMetrics::default();

        let orders = vec![create_order_for_test(), create_order_for_test()];
        let state = AccountState::with_balance_for(&orders);

        let batch = U256::from(42);

        reader
            .expect_get_auction_data()
            .with(eq(batch))
            .times(1)
            .return_once({
                let result = (state.clone(), orders.clone());
                |_| Ok(result)
            });

        submitter
            .expect_get_solution_objective_value()
            .with(eq(batch), always())
            .times(1)
            .returning(|_, _| Ok(U256::from(1337)));

        submitter
            .expect_submit_solution()
            .with(eq(batch), always(), eq(U256::from(1337)))
            .times(1)
            .returning(|_, _, _| Ok(()));

        let solution = Solution {
            prices: map_from_slice(&[(0, 1), (1, 2)]),
            executed_orders: vec![
                order_to_executed_order(&orders[0], 0, 0),
                order_to_executed_order(&orders[1], 2, 2),
            ],
        };
        pf.expect_find_prices()
            .withf(move |o, s| o == orders.as_slice() && *s == state)
            .return_once(move |_, _| Ok(solution));

        let mut driver = test_driver(&mut pf, &reader, &submitter, metrics);

        // First auction
        assert_eq!(
            driver
                .run_internal(SystemTime::UNIX_EPOCH + BATCH_DURATION * 43)
                .unwrap(),
            true
        );

        //Second auction
        assert_eq!(
            driver
                .run_internal(SystemTime::UNIX_EPOCH + BATCH_DURATION * 43)
                .unwrap(),
            false
        );
    }

    #[test]
    fn test_errors_on_failing_reader() {
        let mut reader = MockStableXOrderBookReading::default();
        let submitter = MockStableXSolutionSubmitting::default();
        let mut pf = MockPriceFinding::default();
        let metrics = StableXMetrics::default();

        reader
            .expect_get_auction_data()
            .returning(|_| Err(anyhow!("Error")));

        let mut driver = test_driver(&mut pf, &reader, &submitter, metrics);

        assert!(driver
            .run_internal(SystemTime::UNIX_EPOCH + BATCH_DURATION * 43)
            .is_err())
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

        reader
            .expect_get_auction_data()
            .with(eq(batch))
            .times(1)
            .return_once({
                let result = (state, orders);
                |_| Ok(result)
            });

        submitter
            .expect_get_solution_objective_value()
            .with(eq(batch), always())
            .returning(|_, _| Ok(U256::from(1337)));

        submitter
            .expect_submit_solution()
            .with(eq(batch), always(), eq(U256::from(1337)))
            .returning(|_, _, _| Ok(()));

        pf.expect_find_prices()
            .returning(|_, _| Err(anyhow!("Error")));

        let mut driver = test_driver(&mut pf, &reader, &submitter, metrics);

        assert!(driver
            .run_internal(SystemTime::UNIX_EPOCH + BATCH_DURATION * 43)
            .is_err());
    }

    #[test]
    fn test_do_not_invoke_solver_when_no_orders() {
        let mut reader = MockStableXOrderBookReading::default();
        let submitter = MockStableXSolutionSubmitting::default();
        let mut pf = MockPriceFinding::default();
        let metrics = StableXMetrics::default();

        let orders = vec![];
        let state = AccountState::with_balance_for(&orders);

        let batch = U256::from(42);

        reader
            .expect_get_auction_data()
            .with(eq(batch))
            .times(1)
            .return_once(move |_| Ok((state, orders)));

        let mut driver = test_driver(&mut pf, &reader, &submitter, metrics);
        assert!(driver
            .run_internal(SystemTime::UNIX_EPOCH + BATCH_DURATION * 43)
            .is_ok());
    }

    #[test]
    fn test_do_not_invoke_solver_when_previously_failed() {
        let mut reader = MockStableXOrderBookReading::default();
        let submitter = MockStableXSolutionSubmitting::default();
        let mut pf = MockPriceFinding::default();
        let metrics = StableXMetrics::default();

        let orders = vec![create_order_for_test(), create_order_for_test()];
        let state = AccountState::with_balance_for(&orders);

        let batch = U256::from(42);

        reader
            .expect_get_auction_data()
            .with(eq(batch))
            .times(1)
            .return_once(move |_| Ok((state, orders)));

        pf.expect_find_prices()
            .returning(|_, _| Err(anyhow!("Error")));

        let mut driver = test_driver(&mut pf, &reader, &submitter, metrics);

        // First run fails
        assert!(driver
            .run_internal(SystemTime::UNIX_EPOCH + BATCH_DURATION * 43)
            .is_err());

        // Second run is skipped
        assert_eq!(
            driver
                .run_internal(SystemTime::UNIX_EPOCH + BATCH_DURATION * 43)
                .expect("should have succeeded"),
            false
        );
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

        reader
            .expect_get_auction_data()
            .with(eq(batch))
            .times(1)
            .return_once({
                let result = (state.clone(), orders.clone());
                |_| Ok(result)
            });

        let solution = Solution::trivial();
        pf.expect_find_prices()
            .withf(move |o, s| o == orders.as_slice() && *s == state)
            .return_once(move |_, _| Ok(solution));

        submitter.expect_submit_solution().times(0);

        let mut driver = test_driver(&mut pf, &reader, &submitter, metrics);
        assert!(driver
            .run_internal(SystemTime::UNIX_EPOCH + BATCH_DURATION * 43)
            .is_ok());
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

        reader
            .expect_get_auction_data()
            .with(eq(batch))
            .times(1)
            .return_once({
                let result = (state.clone(), orders.clone());
                |_| Ok(result)
            });

        submitter
            .expect_get_solution_objective_value()
            .with(eq(batch), always())
            .returning(|_, _| {
                Err(SolutionSubmissionError::Unexpected(anyhow!(
                    "get_solution_objective_value failed"
                )))
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
            .withf(move |o, s| o == orders.as_slice() && *s == state)
            .return_once(move |_, _| Ok(solution));

        let mut driver = test_driver(&mut pf, &reader, &submitter, metrics);
        assert!(driver
            .run_internal(SystemTime::UNIX_EPOCH + BATCH_DURATION * 43)
            .is_err());
    }

    #[test]
    fn test_do_not_invoke_solver_when_validation_previously_failed() {
        let mut reader = MockStableXOrderBookReading::default();
        let mut submitter = MockStableXSolutionSubmitting::default();
        let mut pf = MockPriceFinding::default();
        let metrics = StableXMetrics::default();

        let orders = vec![create_order_for_test(), create_order_for_test()];
        let state = AccountState::with_balance_for(&orders);

        let batch = U256::from(42);

        reader
            .expect_get_auction_data()
            .with(eq(batch))
            .times(1)
            .return_once({
                let result = (state.clone(), orders.clone());
                |_| Ok(result)
            });

        submitter
            .expect_get_solution_objective_value()
            .with(eq(batch), always())
            .returning(|_, _| {
                Err(SolutionSubmissionError::Unexpected(anyhow!(
                    "get_solution_objective_value failed"
                )))
            });

        let solution = Solution {
            prices: map_from_slice(&[(0, 1), (1, 2)]),
            executed_orders: vec![
                order_to_executed_order(&orders[0], 0, 0),
                order_to_executed_order(&orders[1], 2, 2),
            ],
        };
        pf.expect_find_prices()
            .withf(move |o, s| o == orders.as_slice() && *s == state)
            .return_once(move |_, _| Ok(solution));

        let mut driver = test_driver(&mut pf, &reader, &submitter, metrics);

        // First run fails
        assert!(driver
            .run_internal(SystemTime::UNIX_EPOCH + BATCH_DURATION * 43)
            .is_err());

        // Second run is skipped
        assert_eq!(
            driver
                .run_internal(SystemTime::UNIX_EPOCH + BATCH_DURATION * 43)
                .expect("should have succeeded"),
            false
        );
    }

    #[test]
    fn test_do_not_invoke_solver_when_submission_previously_failed() {
        let mut reader = MockStableXOrderBookReading::default();
        let mut submitter = MockStableXSolutionSubmitting::default();
        let mut pf = MockPriceFinding::default();
        let metrics = StableXMetrics::default();

        let orders = vec![create_order_for_test(), create_order_for_test()];
        let state = AccountState::with_balance_for(&orders);

        let batch = U256::from(42);

        reader
            .expect_get_auction_data()
            .with(eq(batch))
            .times(1)
            .return_once({
                let result = (state.clone(), orders.clone());
                |_| Ok(result)
            });

        submitter
            .expect_get_solution_objective_value()
            .with(eq(batch), always())
            .returning(|_, _| Ok(U256::from(1337)));

        submitter
            .expect_submit_solution()
            .with(eq(batch), always(), eq(U256::from(1337)))
            .returning(|_, _, _| {
                Err(SolutionSubmissionError::Unexpected(anyhow!(
                    "submit_solution failed"
                )))
            });

        let solution = Solution {
            prices: map_from_slice(&[(0, 1), (1, 2)]),
            executed_orders: vec![
                order_to_executed_order(&orders[0], 0, 0),
                order_to_executed_order(&orders[1], 2, 2),
            ],
        };
        pf.expect_find_prices()
            .withf(move |o, s| o == orders.as_slice() && *s == state)
            .return_once(move |_, _| Ok(solution));

        let mut driver = test_driver(&mut pf, &reader, &submitter, metrics);

        // First run fails
        assert!(driver
            .run_internal(SystemTime::UNIX_EPOCH + BATCH_DURATION * 43)
            .is_err());

        // Second run is skipped
        assert_eq!(
            driver
                .run_internal(SystemTime::UNIX_EPOCH + BATCH_DURATION * 43)
                .expect("should have succeeded"),
            false
        );
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

        reader
            .expect_get_auction_data()
            .with(eq(batch))
            .times(1)
            .return_once({
                let result = (state.clone(), orders.clone());
                |_| Ok(result)
            });

        submitter
            .expect_get_solution_objective_value()
            .with(eq(batch), always())
            .returning(|_, _| Err(SolutionSubmissionError::Benign("Benign Error".to_owned())));
        submitter.expect_submit_solution().times(0);

        let solution = Solution {
            prices: map_from_slice(&[(0, 1), (1, 2)]),
            executed_orders: vec![
                order_to_executed_order(&orders[0], 0, 0),
                order_to_executed_order(&orders[1], 2, 2),
            ],
        };
        pf.expect_find_prices()
            .withf(move |o, s| o == orders.as_slice() && *s == state)
            .return_once(move |_, _| Ok(solution));

        let mut driver = test_driver(&mut pf, &reader, &submitter, metrics);
        assert!(driver
            .run_internal(SystemTime::UNIX_EPOCH + BATCH_DURATION * 43)
            .is_ok());
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

        reader
            .expect_get_auction_data()
            .with(eq(batch))
            .times(1)
            .return_once({
                let result = (state.clone(), orders.clone());
                |_| Ok(result)
            });

        submitter
            .expect_get_solution_objective_value()
            .with(eq(batch), always())
            .returning(|_, _| Ok(42.into()));
        submitter
            .expect_submit_solution()
            .with(eq(batch), always(), eq(U256::from(42)))
            .returning(|_, _, _| Err(SolutionSubmissionError::Benign("Benign Error".to_owned())));

        let solution = Solution {
            prices: map_from_slice(&[(0, 1), (1, 2)]),
            executed_orders: vec![
                order_to_executed_order(&orders[0], 0, 0),
                order_to_executed_order(&orders[1], 2, 2),
            ],
        };
        pf.expect_find_prices()
            .withf(move |o, s| o == orders.as_slice() && *s == state)
            .return_once(move |_, _| Ok(solution));

        let mut driver = StableXDriver::new(
            &mut pf,
            &reader,
            &submitter,
            metrics,
            Duration::from_secs(0),
            BATCH_DURATION,
        );
        assert!(driver
            .run_internal(SystemTime::UNIX_EPOCH + BATCH_DURATION * 43)
            .is_ok());
    }

    #[test]
    pub fn batch_id_current() {
        let start_time = SystemTime::UNIX_EPOCH;
        let batch_id = BatchId::current(start_time).unwrap();
        assert_eq!(batch_id.0, 0);
        assert_eq!(batch_id.start_time(), start_time);

        let start_time = SystemTime::UNIX_EPOCH;
        let batch_id = BatchId::current(start_time + Duration::from_secs(299)).unwrap();
        assert_eq!(batch_id.0, 0);
        assert_eq!(batch_id.start_time(), start_time);

        let start_time = SystemTime::UNIX_EPOCH + Duration::from_secs(300);
        let batch_id = BatchId::current(start_time).unwrap();
        assert_eq!(batch_id.0, 1);
        assert_eq!(batch_id.start_time(), start_time);

        let start_time = SystemTime::UNIX_EPOCH + Duration::from_secs(300);
        let batch_id = BatchId::current(start_time + Duration::from_secs(299)).unwrap();
        assert_eq!(batch_id.0, 1);
        assert_eq!(batch_id.start_time(), start_time);
    }
}
