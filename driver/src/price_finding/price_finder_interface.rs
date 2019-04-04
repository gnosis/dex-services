use crate::models;

use super::error::PriceFindingError;
use web3::types::U256;

#[derive(Clone, Debug)]
pub struct Solution {
    pub surplus: U256,
    pub prices: Vec<u128>,
    pub executed_sell_amounts: Vec<u128>,
    pub executed_buy_amounts: Vec<u128>,
}

impl models::Serializable for Solution {
    fn bytes(&self) -> Vec<u8> {
        [
            &self.prices, 
            &self.executed_sell_amounts,
            &self.executed_buy_amounts,
        ]
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
}