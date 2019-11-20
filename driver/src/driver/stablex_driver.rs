use crate::contracts::stablex_contract::StableXContract;
use crate::error::DriverError;
use crate::price_finding::PriceFinding;

use dfusion_core::models::Solution;

use log::info;

use std::collections::HashSet;

use web3::types::U256;

pub struct StableXDriver<'a> {
    past_auctions: HashSet<U256>,
    contract: &'a dyn StableXContract,
    price_finder: &'a mut dyn PriceFinding,
}

impl<'a> StableXDriver<'a> {
    pub fn new(contract: &'a dyn StableXContract, price_finder: &'a mut dyn PriceFinding) -> Self {
        StableXDriver {
            past_auctions: HashSet::new(),
            contract,
            price_finder,
        }
    }

    pub fn run(&mut self) -> Result<bool, DriverError> {
        // Try to process previous batch auction
        let batch = self.contract.get_current_auction_index()? - 1;
        if self.past_auctions.contains(&batch) {
            return Ok(false);
        }
        let (account_state, orders) = self.contract.get_auction_data(batch)?;
        let solution = if orders.is_empty() {
            info!("No orders in batch {}", batch);
            Solution::trivial(0)
        } else {
            let solution = self.price_finder.find_prices(&orders, &account_state)?;
            info!("Computed solution: {:?}", &solution);
            solution
        };

        let submitted = if solution.is_non_trivial() {
            // NOTE: in retrieving the objective value from the contract the
            //   solution gets validated, ensured that it is better than the
            //   latest submitted solution, and that solutions are still being
            //   accepted for this batch ID.
            let objective_value = self.contract.get_solution_objective_value(
                batch,
                orders.clone(),
                solution.clone(),
            )?;
            info!(
                "Verified solution with objective value: {}",
                objective_value
            );
            self.contract
                .submit_solution(batch, orders, solution, objective_value)?;
            info!("Successfully applied solution to batch {}", batch);
            true
        } else {
            info!("Not submitting trivial solution for batch {}", batch);
            false
        };
        self.past_auctions.insert(batch);
        Ok(submitted)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::contracts::stablex_contract::tests::StableXContractMock;
    use crate::price_finding::price_finder_interface::tests::PriceFindingMock;

    use dfusion_core::models::account_state::test_util::*;
    use dfusion_core::models::order::test_util::create_order_for_test;

    use mock_it::Matcher::{Any, Val};

    #[test]
    fn invokes_solver_with_contract_data_for_unprocessed_auction() {
        let contract = StableXContractMock::default();
        let mut pf = PriceFindingMock::default();

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
            prices: vec![1, 2],
            executed_sell_amounts: vec![0, 2],
            executed_buy_amounts: vec![0, 2],
        };
        pf.find_prices
            .given((orders, state))
            .will_return(Ok(solution));

        let mut driver = StableXDriver::new(&contract, &mut pf);
        assert!(driver.run().unwrap());
    }

    #[test]
    fn does_not_process_previously_processed_auction_again() {
        let contract = StableXContractMock::default();
        let mut pf = PriceFindingMock::default();

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
            prices: vec![1, 2],
            executed_sell_amounts: vec![0, 2],
            executed_buy_amounts: vec![0, 2],
        };
        pf.find_prices
            .given((orders, state))
            .will_return(Ok(solution));

        let mut driver = StableXDriver::new(&contract, &mut pf);

        // First auction
        assert_eq!(driver.run().unwrap(), true);

        //Second auction
        assert_eq!(driver.run().unwrap(), false);
    }

    #[test]
    fn test_errors_on_failing_contract() {
        let contract = StableXContractMock::default();
        let mut pf = PriceFindingMock::default();

        let mut driver = StableXDriver::new(&contract, &mut pf);

        assert!(driver.run().is_err())
    }

    #[test]
    fn test_errors_on_failing_price_finder() {
        let contract = StableXContractMock::default();
        let mut pf = PriceFindingMock::default();

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

        let mut driver = StableXDriver::new(&contract, &mut pf);

        assert!(driver.run().is_err())
    }

    #[test]
    fn test_do_not_invoke_solver_when_no_orders() {
        let contract = StableXContractMock::default();
        let mut pf = PriceFindingMock::default();

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

        let mut driver = StableXDriver::new(&contract, &mut pf);

        assert!(driver.run().is_ok());
        assert!(!mock_it::verify(
            pf.find_prices.was_called_with((orders, state))
        ));
    }

    #[test]
    fn test_does_not_submit_empty_solution() {
        let contract = StableXContractMock::default();
        let mut pf = PriceFindingMock::default();

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

        let mut driver = StableXDriver::new(&contract, &mut pf);
        assert!(driver.run().is_ok());
        assert!(!mock_it::verify(
            contract
                .submit_solution
                .was_called_with((batch - 1, Any, Any, Any))
        ));
    }
}
