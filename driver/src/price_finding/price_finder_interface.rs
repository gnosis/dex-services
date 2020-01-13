use dfusion_core::models;

use super::error::PriceFindingError;

#[derive(Clone)]
pub struct Fee {
    pub token: u16,
    /// Value between [0, 1] mapping from 0% -> 100%
    pub ratio: f64,
}

impl Default for Fee {
    fn default() -> Self {
        Fee {
            token: 0,
            ratio: 0.001,
        }
    }
}

pub trait PriceFinding {
    fn find_prices(
        &self,
        orders: &[models::Order],
        state: &models::AccountState,
    ) -> Result<models::Solution, PriceFindingError>;
}

#[cfg(test)]
pub mod tests {
    use super::super::error::ErrorKind;
    use super::*;
    use dfusion_core::models::util::map_from_slice;
    use dfusion_core::models::Serializable;
    use mock_it::Mock;

    pub struct PriceFindingMock {
        pub find_prices: Mock<
            (Vec<models::Order>, models::AccountState),
            Result<models::Solution, PriceFindingError>,
        >,
    }

    impl Default for PriceFindingMock {
        fn default() -> PriceFindingMock {
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
            &self,
            orders: &[models::Order],
            state: &models::AccountState,
        ) -> Result<models::Solution, PriceFindingError> {
            self.find_prices.called((orders.to_vec(), state.to_owned()))
        }
    }

    #[test]
    fn test_serialize_solution() {
        let solution = models::Solution {
            prices: map_from_slice(&[(0, 1), (1, 2)]),
            executed_sell_amounts: vec![3, 4],
            executed_buy_amounts: vec![5, 6],
        };
        assert_eq!(
            solution.bytes(),
            vec![
                0, 2, // len(prices)
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
