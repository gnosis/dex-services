use crate::metrics::StableXMetrics;
use crate::models::Solution;
use crate::orderbook::StableXOrderBookReading;
use crate::price_finding::PriceFinding;
use crate::solution_submission::{SolutionSubmissionError, StableXSolutionSubmitting};
use anyhow::Result;

use log::info;

use std::collections::HashSet;

use ethcontract::U256;

pub struct StableXDriver<'a> {
    past_auctions: HashSet<U256>,
    price_finder: &'a mut dyn PriceFinding,
    orderbook_reader: &'a dyn StableXOrderBookReading,
    solution_submitter: &'a dyn StableXSolutionSubmitting,
    metrics: StableXMetrics,
}

impl<'a> StableXDriver<'a> {
    pub fn new(
        price_finder: &'a mut dyn PriceFinding,
        orderbook_reader: &'a dyn StableXOrderBookReading,
        solution_submitter: &'a dyn StableXSolutionSubmitting,
        metrics: StableXMetrics,
    ) -> Self {
        StableXDriver {
            past_auctions: HashSet::new(),
            price_finder,
            orderbook_reader,
            solution_submitter,
            metrics,
        }
    }

    #[cfg(test)]
    fn with_past_auction(
        price_finder: &'a mut dyn PriceFinding,
        orderbook_reader: &'a dyn StableXOrderBookReading,
        solution_submitter: &'a dyn StableXSolutionSubmitting,
        metrics: StableXMetrics,
    ) -> Self {
        let mut driver =
            StableXDriver::new(price_finder, orderbook_reader, solution_submitter, metrics);
        driver.past_auctions.insert(0.into());
        driver
    }

    pub fn run(&mut self) -> Result<bool> {
        // Try to process previous batch auction
        let batch_to_solve_result = self.orderbook_reader.get_auction_index();

        if batch_to_solve_result
            .as_ref()
            .map(|batch| self.past_auctions.contains(batch))
            .unwrap_or(false)
        {
            return Ok(false);
        }

        self.metrics
            .auction_processing_started(&batch_to_solve_result);
        let batch_to_solve = batch_to_solve_result?;

        // NOTE: As an interim solution, we skip the first batch so we don't
        //   spill over into the second batch.
        if self.past_auctions.is_empty() {
            self.past_auctions.insert(batch_to_solve);
            self.metrics.auction_ignored();
            return Ok(false);
        }

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
            .expect_get_auction_index()
            .return_once(move || Ok(batch));

        reader
            .expect_get_auction_data()
            .with(eq(batch))
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

        let mut driver = StableXDriver::with_past_auction(&mut pf, &reader, &submitter, metrics);
        assert!(driver.run().unwrap());
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
            .expect_get_auction_index()
            .returning(move || Ok(batch));

        reader
            .expect_get_auction_data()
            .with(eq(batch))
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

        let mut driver = StableXDriver::with_past_auction(&mut pf, &reader, &submitter, metrics);

        // First auction
        assert_eq!(driver.run().unwrap(), true);

        //Second auction
        assert_eq!(driver.run().unwrap(), false);
    }

    #[test]
    fn skips_first_batch() {
        let mut reader = MockStableXOrderBookReading::default();
        let submitter = MockStableXSolutionSubmitting::default();
        let mut pf = MockPriceFinding::default();
        let metrics = StableXMetrics::default();

        let batch = U256::from(42);
        reader
            .expect_get_auction_index()
            .returning(move || Ok(batch));

        let mut driver = StableXDriver::new(&mut pf, &reader, &submitter, metrics);

        assert_eq!(driver.run().unwrap(), false);
    }

    #[test]
    fn test_errors_on_failing_reader() {
        let mut reader = MockStableXOrderBookReading::default();
        let submitter = MockStableXSolutionSubmitting::default();
        let mut pf = MockPriceFinding::default();
        let metrics = StableXMetrics::default();

        reader
            .expect_get_auction_index()
            .returning(|| Err(anyhow!("Error")));

        let mut driver = StableXDriver::with_past_auction(&mut pf, &reader, &submitter, metrics);

        assert!(driver.run().is_err())
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
            .expect_get_auction_index()
            .returning(move || Ok(batch));

        reader
            .expect_get_auction_data()
            .with(eq(batch))
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

        let mut driver = StableXDriver::with_past_auction(&mut pf, &reader, &submitter, metrics);

        assert!(driver.run().is_err());
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
            .expect_get_auction_index()
            .returning(move || Ok(batch));

        reader
            .expect_get_auction_data()
            .with(eq(batch))
            .return_once(move |_| Ok((state, orders)));

        let mut driver = StableXDriver::with_past_auction(&mut pf, &reader, &submitter, metrics);
        assert!(driver.run().is_ok());
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
            .expect_get_auction_index()
            .returning(move || Ok(batch));

        reader
            .expect_get_auction_data()
            .with(eq(batch))
            .return_once(move |_| Ok((state, orders)));

        pf.expect_find_prices()
            .returning(|_, _| Err(anyhow!("Error")));

        let mut driver = StableXDriver::with_past_auction(&mut pf, &reader, &submitter, metrics);

        // First run fails
        assert!(driver.run().is_err());

        // Second run is skipped
        assert_eq!(driver.run().expect("should have succeeded"), false);
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
            .expect_get_auction_index()
            .returning(move || Ok(batch));

        reader
            .expect_get_auction_data()
            .with(eq(batch))
            .return_once({
                let result = (state.clone(), orders.clone());
                |_| Ok(result)
            });

        let solution = Solution::trivial();
        pf.expect_find_prices()
            .withf(move |o, s| o == orders.as_slice() && *s == state)
            .return_once(move |_, _| Ok(solution));

        submitter.expect_submit_solution().times(0);

        let mut driver = StableXDriver::with_past_auction(&mut pf, &reader, &submitter, metrics);
        assert!(driver.run().is_ok());
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
            .expect_get_auction_index()
            .returning(move || Ok(batch));

        reader
            .expect_get_auction_data()
            .with(eq(batch))
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

        let mut driver = StableXDriver::with_past_auction(&mut pf, &reader, &submitter, metrics);
        assert!(driver.run().is_err());
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
            .expect_get_auction_index()
            .returning(move || Ok(batch));

        reader
            .expect_get_auction_data()
            .with(eq(batch))
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

        let mut driver = StableXDriver::with_past_auction(&mut pf, &reader, &submitter, metrics);

        // First run fails
        assert!(driver.run().is_err());

        // Second run is skipped
        assert_eq!(driver.run().expect("should have succeeded"), false);
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
            .expect_get_auction_index()
            .returning(move || Ok(batch));

        reader
            .expect_get_auction_data()
            .with(eq(batch))
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

        let mut driver = StableXDriver::with_past_auction(&mut pf, &reader, &submitter, metrics);

        // First run fails
        assert!(driver.run().is_err());

        // Second run is skipped
        assert_eq!(driver.run().expect("should have succeeded"), false);
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
            .expect_get_auction_index()
            .returning(move || Ok(batch));

        reader
            .expect_get_auction_data()
            .with(eq(batch))
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

        let mut driver = StableXDriver::with_past_auction(&mut pf, &reader, &submitter, metrics);
        assert!(driver.run().is_ok());
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
            .expect_get_auction_index()
            .returning(move || Ok(batch));

        reader
            .expect_get_auction_data()
            .with(eq(batch))
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

        let mut driver = StableXDriver::new(&mut pf, &reader, &submitter, metrics);
        assert!(driver.run().is_ok());
    }
}
