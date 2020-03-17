use crate::metrics::StableXMetrics;
use crate::models::{account_state::AccountState, order::Order, Solution};
use crate::orderbook::StableXOrderBookReading;
use crate::price_finding::PriceFinding;
use crate::solution_submission::{SolutionSubmissionError, StableXSolutionSubmitting};
use anyhow::{Error, Result};
use ethcontract::U256;
use log::info;

#[derive(Debug)]
pub enum DriverResult {
    Ok,
    Retry(Error),
    Skip(Error),
}

#[cfg_attr(test, mockall::automock)]
pub trait StableXDriver {
    fn run(&mut self, batch_to_solve: U256) -> DriverResult;
}

pub struct StableXDriverImpl<'a> {
    price_finder: &'a mut dyn PriceFinding,
    orderbook_reader: &'a dyn StableXOrderBookReading,
    solution_submitter: &'a dyn StableXSolutionSubmitting,
    metrics: &'a StableXMetrics,
}

impl<'a> StableXDriverImpl<'a> {
    pub fn new(
        price_finder: &'a mut dyn PriceFinding,
        orderbook_reader: &'a dyn StableXOrderBookReading,
        solution_submitter: &'a dyn StableXSolutionSubmitting,
        metrics: &'a StableXMetrics,
    ) -> Self {
        Self {
            price_finder,
            orderbook_reader,
            solution_submitter,
            metrics,
        }
    }

    fn get_orderbook(&mut self, batch_to_solve: U256) -> Result<(AccountState, Vec<Order>)> {
        let get_auction_data_result = self.orderbook_reader.get_auction_data(batch_to_solve);
        self.metrics
            .auction_orders_fetched(batch_to_solve, &get_auction_data_result);
        get_auction_data_result
    }

    fn solve(
        &mut self,
        batch_to_solve: U256,
        account_state: AccountState,
        orders: Vec<Order>,
    ) -> Result<()> {
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

        Ok(())
    }
}

impl<'a> StableXDriver for StableXDriverImpl<'a> {
    fn run(&mut self, batch_to_solve: U256) -> DriverResult {
        self.metrics.auction_processing_started(&Ok(batch_to_solve));
        let (account_state, orders) = match self.get_orderbook(batch_to_solve) {
            Ok(ok) => ok,
            Err(err) => return DriverResult::Retry(err),
        };
        match self.solve(batch_to_solve, account_state, orders) {
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
    use mockall::predicate::*;

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

        let mut driver = StableXDriverImpl::new(&mut pf, &reader, &submitter, &metrics);
        assert!(driver.run(batch).is_ok());
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

        let mut driver = StableXDriverImpl::new(&mut pf, &reader, &submitter, &metrics);

        assert!(driver.run(U256::from(42)).is_retry())
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

        let mut driver = StableXDriverImpl::new(&mut pf, &reader, &submitter, &metrics);

        assert!(driver.run(batch).is_skip());
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
            .return_once(move |_| Ok((state, orders)));

        let mut driver = StableXDriverImpl::new(&mut pf, &reader, &submitter, &metrics);
        assert!(driver.run(batch).is_ok());
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
            .return_once({
                let result = (state.clone(), orders.clone());
                |_| Ok(result)
            });

        let solution = Solution::trivial();
        pf.expect_find_prices()
            .withf(move |o, s| o == orders.as_slice() && *s == state)
            .return_once(move |_, _| Ok(solution));

        submitter.expect_submit_solution().times(0);

        let mut driver = StableXDriverImpl::new(&mut pf, &reader, &submitter, &metrics);
        assert!(driver.run(batch).is_ok());
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

        let mut driver = StableXDriverImpl::new(&mut pf, &reader, &submitter, &metrics);
        assert!(driver.run(batch).is_skip());
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

        let mut driver = StableXDriverImpl::new(&mut pf, &reader, &submitter, &metrics);
        assert!(driver.run(batch).is_ok());
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

        let mut driver = StableXDriverImpl::new(&mut pf, &reader, &submitter, &metrics);
        assert!(driver.run(batch).is_ok());
    }
}
