use dfusion_core::models;

use super::error::PriceFindingError;
use std::iter::once;
use web3::types::U256;

#[derive(Clone, Debug, PartialEq)]
pub struct Solution {
    pub surplus: U256,
    pub prices: Vec<u128>,
    pub executed_sell_amounts: Vec<u128>,
    pub executed_buy_amounts: Vec<u128>,
}

impl models::Serializable for Solution {
    fn bytes(&self) -> Vec<u8> {
        let alternating_buy_sell_amounts: Vec<u128> = self.executed_buy_amounts
            .iter()
            .zip(self.executed_sell_amounts.iter())
            .flat_map(|tup| once(tup.0).chain(once(tup.1)))
            .cloned()
            .collect();
        [&self.prices, &alternating_buy_sell_amounts]
            .iter()
            .flat_map(|list| list.iter())
            .flat_map(models::Serializable::bytes)
            .collect()
    }
}

pub trait PriceFinding {
    fn find_prices(
        &mut self, 
        orders: &[models::Order],
        state: &models::State
    ) -> Result<Solution, PriceFindingError>;
}

#[cfg(test)]
pub mod tests {
    extern crate mock_it;
    
    use super::*;
    use dfusion_core::models::Serializable;
    use mock_it::Mock;
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
            orders: &[models::Order],
            state: &models::State
        ) -> Result<Solution, PriceFindingError> {
            self.find_prices.called((orders.to_vec(), state.to_owned()))
        }
    }

    #[test]
    fn test_serialize_solution() {
        let solution = Solution {
            surplus: U256::zero(),
            prices: vec![1,2],
            executed_sell_amounts: vec![3, 4],
            executed_buy_amounts: vec![5, 6],
        };
        assert_eq!(solution.bytes(), vec![
            0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, // p1
            0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 2, // p2
            0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 5, // b1
            0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 3, // s1
            0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 6, // b2
            0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 4, // s2
        ]);
    }
}
