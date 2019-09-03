use dfusion_core::models;

use super::error::PriceFindingError;

pub trait PriceFinding {
    fn find_prices(
        &mut self,
        orders: &[models::Order],
        state: &models::AccountState,
    ) -> Result<models::Solution, PriceFindingError>;
}

#[cfg(test)]
pub mod tests {
    extern crate mock_it;

    use super::super::error::ErrorKind;
    use super::*;
    use dfusion_core::models::Serializable;
    use mock_it::Mock;
    use web3::types::U256;

    pub struct PriceFindingMock {
        pub find_prices: Mock<
            (Vec<models::Order>, models::AccountState),
            Result<models::Solution, PriceFindingError>,
        >,
    }

    impl PriceFindingMock {
        pub fn default() -> PriceFindingMock {
            PriceFindingMock {
                find_prices: Mock::new(Err(PriceFindingError::new(
                    "Unexpected call to find_prices",
                    ErrorKind::Unknown,
                ))),
            }
        }
    }

    impl PriceFinding for PriceFindingMock {
        fn find_prices(
            &mut self,
            orders: &[models::Order],
            state: &models::AccountState,
        ) -> Result<models::Solution, PriceFindingError> {
            self.find_prices.called((orders.to_vec(), state.to_owned()))
        }
    }

    #[test]
    fn test_serialize_solution() {
        let solution = models::Solution {
            surplus: Some(U256::zero()),
            prices: vec![1, 2],
            executed_sell_amounts: vec![3, 4],
            executed_buy_amounts: vec![5, 6],
        };
        assert_eq!(
            solution.bytes(),
            vec![
                0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, // p1
                0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 2, // p2
                0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 5, // b1
                0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 3, // s1
                0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 6, // b2
                0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 4, // s2
            ]
        );
    }
}
