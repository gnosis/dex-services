use crate::models;

use crate::error::DriverError;
use web3::types::U256;

#[derive(Clone)]
pub struct Solution {
    pub surplus: U256,
    pub prices: Vec<u128>,
    pub executed_sell_amounts: Vec<u128>,
    pub executed_buy_amounts: Vec<u128>,
}

pub trait PriceFinding {
    fn find_prices(
        &self, 
        orders: Vec<models::Order>, 
        state: models::State
    ) -> Result<Solution, DriverError>;
}

#[cfg(test)]
pub mod tests {
    extern crate mock_it;

    use super::*;
    use mock_it::Mock;
    use crate::error::ErrorKind;

    pub struct PriceFindingMock {
        pub find_prices: Mock<(Vec<models::Order>, models::State), Result<Solution, DriverError>>,
    }

    impl PriceFindingMock {
        pub fn new() -> PriceFindingMock {
            PriceFindingMock {
                find_prices: Mock::new(Err(DriverError::new("Unexpected call to find_prices", ErrorKind::Unknown))),
            }
        }
    }

    impl PriceFinding for PriceFindingMock {
        fn find_prices(
            &self, 
            orders: Vec<models::Order>, 
            state: models::State
        ) -> Result<Solution, DriverError> {
            self.find_prices.called((orders, state))
        }
    }
}