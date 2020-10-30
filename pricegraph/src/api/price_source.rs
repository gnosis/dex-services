//! This module implements price source methods for the `Pricegraph` API so that
//! it can be used for OWL price estimates to the solver.

use crate::encoding::{TokenId, TokenPair, TokenPairRange};
use crate::{Pricegraph, FEE_TOKEN};

const OWL_BASE_UNIT: f64 = 1_000_000_000_000_000_000.0;

impl Pricegraph {
    /// Estimates the fee token price in atoms for the specified token.
    /// Specifically, this price repesents the number of OWL atoms required to
    /// buy 1e18 atoms of the specified token.
    ///
    /// Returns `None` if the token is not connected to the fee token (that is,
    /// there is no transitive order buying the fee token for the specified
    /// token).
    ///
    /// The fee token is defined as the token with ID 0.
    pub fn estimate_token_price(&self, token: TokenId, hops: Option<usize>) -> Option<f64> {
        if token == FEE_TOKEN {
            return Some(OWL_BASE_UNIT);
        }

        // NOTE: Estimate price of selling 1 unit of the reference token for the
        // specified token. We sell rather than buy the reference token because
        // volume is denominated in the sell token, for which we know the number
        // of decimals.
        let pair = TokenPair {
            buy: token,
            sell: FEE_TOKEN,
        };
        let range = TokenPairRange {
            pair,
            hops
        };

        let price_in_token = self.estimate_limit_price(range, OWL_BASE_UNIT)?;
        let price_in_reference = 1.0 / price_in_token;

        Some(OWL_BASE_UNIT * price_in_reference)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::num;
    use crate::test::prelude::*;
    use crate::FEE_FACTOR;

    #[test]
    fn fee_token_is_one_base_unit() {
        let pricegraph = pricegraph! {
            users {}
            orders {}
        };

        assert_approx_eq!(pricegraph.estimate_token_price(0, None).unwrap(), OWL_BASE_UNIT);
    }

    #[test]
    fn estimates_correct_token_price() {
        const LOTS: u128 = 100 * OWL_BASE_UNIT as u128;

        //   /-------0.5--------\
        //  /                    v
        // 0 --1.0--> 1 <--1.0-- 2
        let pricegraph = pricegraph! {
            users {
                @1 {
                    token 1 => LOTS,
                    token 2 => LOTS,
                }
                @2 {
                    token 1 => LOTS,
                }
            }
            orders {
                owner @1 buying 0 [LOTS    ] selling 1 [LOTS],
                owner @1 buying 0 [LOTS / 2] selling 2 [LOTS],
                owner @2 buying 2 [LOTS    ] selling 1 [LOTS],
            }
        };
        let rounding_error = num::max_rounding_error_with_epsilon(OWL_BASE_UNIT);

        assert_approx_eq!(
            pricegraph.estimate_token_price(1, None).unwrap(),
            (OWL_BASE_UNIT / 2.0) * FEE_FACTOR.powi(3),
            rounding_error
        );
        assert_approx_eq!(
            pricegraph.estimate_token_price(2, None).unwrap(),
            (OWL_BASE_UNIT / 2.0) * FEE_FACTOR.powi(2),
            rounding_error
        );
    }
}
