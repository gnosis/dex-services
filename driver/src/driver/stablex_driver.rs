use crate::contracts::stablex_contract::StableXContract;
use crate::error::DriverError;
use crate::price_finding::PriceFinding;

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
            info!("Already processed batch {}", batch);
            return Ok(false);
        }
        let (account_state, orders) = self.contract.get_auction_data(batch)?;
        let solution = self.price_finder.find_prices(&orders, &account_state)?;

        self.contract.submit_solution(batch, orders, solution)?;
        self.past_auctions.insert(batch);
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::contracts::stablex_contract::tests::StableXContractMock;
    use crate::price_finding::price_finder_interface::tests::PriceFindingMock;

    use dfusion_core::models::account_state::test_util::*;
    use dfusion_core::models::order::test_util::create_order_for_test;
    use dfusion_core::models::Solution;

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
<<<<<<< HEAD
<<<<<<< HEAD
            .given(batch - 1)
=======
            .given(batch)
>>>>>>> [StableX] Driver component
=======
            .given(batch - 1)
>>>>>>> Using previous auction
            .will_return(Ok((state.clone(), orders.clone())));

        contract
            .submit_solution
<<<<<<< HEAD
<<<<<<< HEAD
            .given((batch - 1, Val(orders.clone()), Any))
=======
            .given((batch, Val(orders.clone()), Any))
>>>>>>> [StableX] Driver component
=======
            .given((batch - 1, Val(orders.clone()), Any))
>>>>>>> Using previous auction
            .will_return(Ok(()));

        let solution = Solution {
            surplus: Some(U256::zero()),
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
<<<<<<< HEAD
<<<<<<< HEAD
            .given(batch - 1)
=======
            .given(batch)
>>>>>>> [StableX] Driver component
=======
            .given(batch - 1)
>>>>>>> Using previous auction
            .will_return(Ok((state.clone(), orders.clone())));

        contract
            .submit_solution
<<<<<<< HEAD
<<<<<<< HEAD
            .given((batch - 1, Val(orders.clone()), Any))
=======
            .given((batch, Val(orders.clone()), Any))
>>>>>>> [StableX] Driver component
=======
            .given((batch - 1, Val(orders.clone()), Any))
>>>>>>> Using previous auction
            .will_return(Ok(()));

        let solution = Solution {
            surplus: Some(U256::zero()),
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
<<<<<<< HEAD
<<<<<<< HEAD
            .given(batch - 1)
=======
            .given(batch)
>>>>>>> [StableX] Driver component
=======
            .given(batch - 1)
>>>>>>> Using previous auction
            .will_return(Ok((state.clone(), orders.clone())));

        contract
            .submit_solution
<<<<<<< HEAD
<<<<<<< HEAD
            .given((batch - 1, Val(orders.clone()), Any))
=======
            .given((batch, Val(orders.clone()), Any))
>>>>>>> [StableX] Driver component
=======
            .given((batch - 1, Val(orders.clone()), Any))
>>>>>>> Using previous auction
            .will_return(Ok(()));

        let mut driver = StableXDriver::new(&contract, &mut pf);

        assert!(driver.run().is_err())
    }
}
