use crate::contracts::stablex_contract::StableXContract;
use crate::error::DriverError;
use crate::metrics::StableXMetrics;
use crate::price_finding::PriceFinding;

use dfusion_core::models::Solution;

use log::info;

use std::collections::HashSet;

use web3::types::U256;

pub struct StableXDriver<'a> {
    past_auctions: HashSet<U256>,
    contract: &'a dyn StableXContract,
    price_finder: &'a mut dyn PriceFinding,
    metrics: StableXMetrics,
}

impl<'a> StableXDriver<'a> {
    pub fn new(
        contract: &'a dyn StableXContract,
        price_finder: &'a mut dyn PriceFinding,
        metrics: StableXMetrics,
    ) -> Self {
        StableXDriver {
            past_auctions: HashSet::new(),
            contract,
            price_finder,
            metrics,
        }
    }

    pub fn run(&mut self) -> Result<bool, DriverError> {
        // Try to process previous batch auction
        let batch_to_solve_result = self
            .contract
            .get_current_auction_index()
            .map(|batch_collecting_orders| batch_collecting_orders - 1);

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

        let get_auction_data_result = self.contract.get_auction_data(batch_to_solve);
        self.metrics
            .auction_orders_fetched(batch_to_solve, &get_auction_data_result);
        let (account_state, orders) = get_auction_data_result?;

        let solution = if orders.is_empty() {
            info!("No orders in batch {}", batch_to_solve);
            Solution::trivial(0)
        } else {
            let price_finder_result = self.price_finder.find_prices(&orders, &account_state);
            self.metrics
                .auction_solution_computed(batch_to_solve, &orders, &price_finder_result);
            match price_finder_result {
                Ok(solution) => {
                    info!("Computed solution: {:?}", &solution);
                    solution
                }
                Err(err) => {
                    self.past_auctions.insert(batch_to_solve);
                    return Err(err.into());
                }
            }
        };

        let submitted = if solution.is_non_trivial() {
            // NOTE: in retrieving the objective value from the contract the
            //   solution gets validated, ensured that it is better than the
            //   latest submitted solution, and that solutions are still being
            //   accepted for this batch ID.
            let verification_result = self.contract.get_solution_objective_value(
                batch_to_solve,
                orders.clone(),
                solution.clone(),
            );
            self.metrics
                .auction_solution_verified(batch_to_solve, &verification_result);

            let objective_value = verification_result?;
            info!(
                "Verified solution with objective value: {}",
                objective_value
            );
            let submission_result =
                self.contract
                    .submit_solution(batch_to_solve, orders, solution, objective_value);
            self.metrics
                .auction_solution_submitted(batch_to_solve, &submission_result);
            submission_result?;

            info!("Successfully applied solution to batch {}", batch_to_solve);
            true
        } else {
            info!(
                "Not submitting trivial solution for batch {}",
                batch_to_solve
            );
            self.metrics.auction_skipped(batch_to_solve);
            false
        };
        self.past_auctions.insert(batch_to_solve);
        Ok(submitted)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::contracts::stablex_contract::tests::StableXContractMock;
    use crate::error::ErrorKind;
    use crate::price_finding::price_finder_interface::tests::PriceFindingMock;

    use dfusion_core::models::account_state::test_util::*;
    use dfusion_core::models::order::test_util::create_order_for_test;
    use dfusion_core::models::util::map_from_slice;

    use mock_it::Matcher::{Any, Val};

    #[test]
    fn invokes_solver_with_contract_data_for_unprocessed_auction() {
        let contract = StableXContractMock::default();
        let mut pf = PriceFindingMock::default();
        let metrics = StableXMetrics::default();

        let orders = vec![create_order_for_test(), create_order_for_test()];
        let state = create_account_state_with_balance_for(&orders);

        let batch = U256::from(42);
        contract
            .get_current_auction_index
            .given(())
            .will_return(Ok(batch));

        contract
            .get_auction_data
            .given(batch - 1)
            .will_return(Ok((state.clone(), orders.clone())));

        contract
            .get_solution_objective_value
            .given((batch - 1, Val(orders.clone()), Any))
            .will_return(Ok(U256::from(1337)));

        contract
            .submit_solution
            .given((batch - 1, Val(orders.clone()), Any, Val(U256::from(1337))))
            .will_return(Ok(()));

        let solution = Solution {
            prices: map_from_slice(&[(0, 1), (1, 2)]),
            executed_sell_amounts: vec![0, 2],
            executed_buy_amounts: vec![0, 2],
        };
        pf.find_prices
            .given((orders, state))
            .will_return(Ok(solution));

        let mut driver = StableXDriver::new(&contract, &mut pf, metrics);
        assert!(driver.run().unwrap());
    }

    #[test]
    fn does_not_process_previously_processed_auction_again() {
        let contract = StableXContractMock::default();
        let mut pf = PriceFindingMock::default();
        let metrics = StableXMetrics::default();

        let orders = vec![create_order_for_test(), create_order_for_test()];
        let state = create_account_state_with_balance_for(&orders);

        let batch = U256::from(42);
        contract
            .get_current_auction_index
            .given(())
            .will_return(Ok(batch));

        contract
            .get_auction_data
            .given(batch - 1)
            .will_return(Ok((state.clone(), orders.clone())));

        contract
            .get_solution_objective_value
            .given((batch - 1, Val(orders.clone()), Any))
            .will_return(Ok(U256::from(1337)));

        contract
            .submit_solution
            .given((batch - 1, Val(orders.clone()), Any, Val(U256::from(1337))))
            .will_return(Ok(()));

        let solution = Solution {
            prices: map_from_slice(&[(0, 1), (1, 2)]),
            executed_sell_amounts: vec![0, 2],
            executed_buy_amounts: vec![0, 2],
        };
        pf.find_prices
            .given((orders, state))
            .will_return(Ok(solution));

        let mut driver = StableXDriver::new(&contract, &mut pf, metrics);

        // First auction
        assert_eq!(driver.run().unwrap(), true);

        //Second auction
        assert_eq!(driver.run().unwrap(), false);
    }

    #[test]
    fn test_errors_on_failing_contract() {
        let contract = StableXContractMock::default();
        let mut pf = PriceFindingMock::default();
        let metrics = StableXMetrics::default();

        let mut driver = StableXDriver::new(&contract, &mut pf, metrics);

        assert!(driver.run().is_err())
    }

    #[test]
    fn test_errors_on_failing_price_finder() {
        let contract = StableXContractMock::default();
        let mut pf = PriceFindingMock::default();
        let metrics = StableXMetrics::default();

        let orders = vec![create_order_for_test(), create_order_for_test()];
        let state = create_account_state_with_balance_for(&orders);

        let batch = U256::from(42);
        contract
            .get_current_auction_index
            .given(())
            .will_return(Ok(batch));

        contract
            .get_auction_data
            .given(batch - 1)
            .will_return(Ok((state, orders.clone())));

        contract
            .get_solution_objective_value
            .given((batch - 1, Val(orders.clone()), Any))
            .will_return(Ok(U256::from(1337)));

        contract
            .submit_solution
            .given((batch - 1, Val(orders), Any, Val(U256::from(1337))))
            .will_return(Ok(()));

        let mut driver = StableXDriver::new(&contract, &mut pf, metrics);

        assert!(driver.run().is_err())
    }

    #[test]
    fn test_do_not_invoke_solver_when_no_orders() {
        let contract = StableXContractMock::default();
        let mut pf = PriceFindingMock::default();
        let metrics = StableXMetrics::default();

        let orders = vec![];
        let state = create_account_state_with_balance_for(&orders);

        let batch = U256::from(42);
        contract
            .get_current_auction_index
            .given(())
            .will_return(Ok(batch));

        contract
            .get_auction_data
            .given(batch - 1)
            .will_return(Ok((state.clone(), orders.clone())));

        let mut driver = StableXDriver::new(&contract, &mut pf, metrics);

        assert!(driver.run().is_ok());
        assert!(!mock_it::verify(
            pf.find_prices.was_called_with((orders, state))
        ));
    }

    #[test]
    fn test_do_not_invoke_solver_when_previously_failed() {
        let contract = StableXContractMock::default();
        let mut pf = PriceFindingMock::default();
        let metrics = StableXMetrics::default();

        let orders = vec![create_order_for_test(), create_order_for_test()];
        let state = create_account_state_with_balance_for(&orders);

        let batch = U256::from(42);
        contract
            .get_current_auction_index
            .given(())
            .will_return(Ok(batch));

        contract
            .get_auction_data
            .given(batch - 1)
            .will_return(Ok((state.clone(), orders.clone())));

        let mut driver = StableXDriver::new(&contract, &mut pf, metrics);

        // First run fails
        assert!(driver.run().is_err());

        // Second run is skipped
        assert_eq!(driver.run().expect("should have succeeded"), false);
        assert!(mock_it::verify(
            pf.find_prices.was_called_with((orders, state)).times(1)
        ));
    }

    #[test]
    fn test_does_not_submit_empty_solution() {
        let contract = StableXContractMock::default();
        let mut pf = PriceFindingMock::default();
        let metrics = StableXMetrics::default();

        let orders = vec![create_order_for_test(), create_order_for_test()];
        let state = create_account_state_with_balance_for(&orders);

        let batch = U256::from(42);
        contract
            .get_current_auction_index
            .given(())
            .will_return(Ok(batch));

        contract
            .get_auction_data
            .given(batch - 1)
            .will_return(Ok((state.clone(), orders.clone())));

        let solution = Solution::trivial(orders.len());
        pf.find_prices
            .given((orders, state))
            .will_return(Ok(solution));

        let mut driver = StableXDriver::new(&contract, &mut pf, metrics);
        assert!(driver.run().is_ok());
        assert!(!mock_it::verify(
            contract
                .submit_solution
                .was_called_with((batch - 1, Any, Any, Any))
        ));
    }
    #[test]
    fn test_does_not_submit_solution_for_which_validation_failed() {
        let contract = StableXContractMock::default();
        let mut pf = PriceFindingMock::default();
        let metrics = StableXMetrics::default();

        let orders = vec![create_order_for_test(), create_order_for_test()];
        let state = create_account_state_with_balance_for(&orders);

        let batch = U256::from(42);
        contract
            .get_current_auction_index
            .given(())
            .will_return(Ok(batch));

        contract
            .get_auction_data
            .given(batch - 1)
            .will_return(Ok((state.clone(), orders.clone())));

        contract
            .get_solution_objective_value
            .given((batch - 1, Val(orders.clone()), Any))
            .will_return(Err(DriverError::new(
                "get_solution_objective_value failed",
                ErrorKind::Unknown,
            )));

        let solution = Solution {
            prices: map_from_slice(&[(0, 1), (1, 2)]),
            executed_sell_amounts: vec![0, 2],
            executed_buy_amounts: vec![0, 2],
        };
        pf.find_prices
            .given((orders, state))
            .will_return(Ok(solution));

        let mut driver = StableXDriver::new(&contract, &mut pf, metrics);
        assert!(driver.run().is_err());
        assert!(!mock_it::verify(
            contract
                .submit_solution
                .was_called_with((batch - 1, Any, Any, Any))
        ));
    }
}
