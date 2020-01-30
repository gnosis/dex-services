use crate::contracts::stablex_contract::StableXContract;
use crate::error::DriverError;

use dfusion_core::models::{Order, Solution};

use web3::types::U256;

type Result<T> = std::result::Result<T, DriverError>;

pub trait StableXSolutionSubmitting {
    /// Return the objective value for the given solution in the given
    /// batch or an error.
    ///
    /// # Arguments
    /// * `batch_index` - the auction for which this solutions should be evaluated
    /// * `orders` - the list of orders for which this solution is applicable
    /// * `solution` - the solution to be evaluated
    fn get_solution_objective_value(
        &self,
        batch_index: U256,
        orders: Vec<Order>,
        solution: Solution,
    ) -> Result<U256>;

    /// Submits the provided solution and returns the result of the submission
    ///
    /// # Arguments
    /// * `batch_index` - the auction for which this solutions should be evaluated
    /// * `orders` - the list of orders for which this solution is applicable
    /// * `solution` - the solution to be evaluated
    /// * `claimed_objective_value` - the objective value of the provided solution.
    fn submit_solution(
        &self,
        batch_index: U256,
        orders: Vec<Order>,
        solution: Solution,
        claimed_objective_value: U256,
    ) -> Result<()>;
}

pub struct StableXSolutionSubmitter<'a> {
    contract: &'a dyn StableXContract,
}

impl<'a> StableXSolutionSubmitter<'a> {
    pub fn new(contract: &'a dyn StableXContract) -> Self {
        Self { contract }
    }
}

impl<'a> StableXSolutionSubmitting for StableXSolutionSubmitter<'a> {
    fn get_solution_objective_value(
        &self,
        batch_index: U256,
        orders: Vec<Order>,
        solution: Solution,
    ) -> Result<U256> {
        self.contract
            .get_solution_objective_value(batch_index, orders, solution)
    }

    fn submit_solution(
        &self,
        batch_index: U256,
        orders: Vec<Order>,
        solution: Solution,
        claimed_objective_value: U256,
    ) -> Result<()> {
        self.contract
            .submit_solution(batch_index, orders, solution, claimed_objective_value)
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use crate::error::ErrorKind;
    use mock_it::{Matcher, Mock};

    type GetSolutionObjectiveValueArguments = (U256, Matcher<Vec<Order>>, Matcher<Solution>);
    type SubmitSolutionArguments = (U256, Matcher<Vec<Order>>, Matcher<Solution>, Matcher<U256>);

    #[derive(Clone)]
    pub struct StableXSolutionSubmittingMock {
        pub get_solution_objective_value: Mock<GetSolutionObjectiveValueArguments, Result<U256>>,
        pub submit_solution: Mock<SubmitSolutionArguments, Result<()>>,
    }

    impl Default for StableXSolutionSubmittingMock {
        fn default() -> Self {
            Self {
                get_solution_objective_value: Mock::new(Err(DriverError::new(
                    "Unexpected call to get_solution_objective_value",
                    ErrorKind::Unknown,
                ))),
                submit_solution: Mock::new(Err(DriverError::new(
                    "Unexpected call to submit_solution",
                    ErrorKind::Unknown,
                ))),
            }
        }
    }

    impl StableXSolutionSubmitting for StableXSolutionSubmittingMock {
        fn get_solution_objective_value(
            &self,
            batch_index: U256,
            orders: Vec<Order>,
            solution: Solution,
        ) -> Result<U256> {
            self.get_solution_objective_value.called((
                batch_index,
                Matcher::Val(orders),
                Matcher::Val(solution),
            ))
        }
        fn submit_solution(
            &self,
            batch_index: U256,
            orders: Vec<Order>,
            solution: Solution,
            claimed_objective_value: U256,
        ) -> Result<()> {
            self.submit_solution.called((
                batch_index,
                Matcher::Val(orders),
                Matcher::Val(solution),
                Matcher::Val(claimed_objective_value),
            ))
        }
    }
}
