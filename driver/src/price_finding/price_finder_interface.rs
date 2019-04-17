use crate::models;

use super::error::PriceFindingError;
use web3::types::U256;
use std::iter::once;


#[derive(Clone, Debug, PartialEq)]
pub struct Solution {
    pub surplus: U256,
    pub prices: Vec<u128>,
    pub executed_sell_amounts: Vec<u128>,
    pub executed_buy_amounts: Vec<u128>,
}

impl models::Serializable for Solution {
    fn bytes(&self) -> Vec<u8> {
        // TODO: need to find better zipping formulation
        let altering_sell_buy_amounts: Vec<u128> = self.executed_sell_amounts
        .iter()
        .zip(self.executed_buy_amounts.iter())
        .flat_map(|tup| once(tup.0).chain(once(tup.1)))
        .map(|x| x.clone())
        .collect();
        [&self.prices, &altering_sell_buy_amounts]
            .iter()
            .flat_map(|list| list.iter())
            .flat_map(|element| element.bytes())
            .collect()
    }
}

pub trait PriceFinding {
    fn find_prices(
        &mut self, 
        orders: &Vec<models::Order>, 
        state: &models::State
    ) -> Result<Solution, PriceFindingError>;
}

#[cfg(test)]
pub mod tests {
    extern crate mock_it;
    
    use super::*;
    use mock_it::Mock;
    use web3::types::U256;
    use crate::models::Serializable;
    use super::super::error::ErrorKind;

    pub struct PriceFindingMock {
        pub find_prices: Mock<(Vec<models::Order>, models::State), Result<Solution, PriceFindingError>>,
    }

    impl PriceFindingMock {
        pub fn new() -> PriceFindingMock {
            PriceFindingMock {
                find_prices: Mock::new(Err(PriceFindingError::new("Unexpected call to find_prices", ErrorKind::Unknown))),
            }
        }
    }

    impl PriceFinding for PriceFindingMock {
        fn find_prices(
            &mut self, 
            orders: &Vec<models::Order>, 
            state: &models::State
        ) -> Result<Solution, PriceFindingError> {
            self.find_prices.called((orders.to_vec(), state.to_owned()))
        }
    }
    #[test]
    fn test_serialization_of_solution(){
        let p = vec![1,3,4];
        let e_s_a = vec![2,5,5,72,0,1];
        let e_b_a = vec![4,4,5,72,0,1];
        let solution = Solution {
            surplus: U256::zero(),
            prices: p,
            executed_sell_amounts: e_s_a,
            executed_buy_amounts: e_b_a,
        };
        let serialized_solution = vec![0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 3, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 4, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 2, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 4, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 5, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 4, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 5, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 5, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 72, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 72, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1];
        assert_eq!(solution.bytes(), serialized_solution);
    }
}