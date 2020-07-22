//! This module implements price source methods for the `Pricegraph` API so that
//! it can be used for OWL price estimates to the solver.

use crate::encoding::{TokenId, TokenPair};
use crate::{Pricegraph, FEE_TOKEN};
use std::cmp;

impl Pricegraph {
    /// Estimates the fee token price in WEI for the specified token. Returns
    /// `None` if the token is not connected to the fee token.
    ///
    /// The fee token is defined as the token with ID 0.
    pub fn token_price_spread(&self, token: TokenId) -> Option<(f64, f64)> {
        if token == FEE_TOKEN {
            return Some((1.0, 1.0));
        }

        let mut orderbook = self.full_orderbook();

        let mut bounds = None;
        while let Some((min, max)) = orderbook.fill_ring_trade_around_token(token) {
            let (current_min, current_max) = *bounds.get_or_insert((min, max));
            bounds = Some((cmp::min(current_min, min), cmp::max(current_max, max)));
        }

        let (min, max) = bounds.or_else(|| {
            // NOTE: If the graph is fully reduced, then just take the inverse
            // exchange rate for selling an epsilon of reference token. Note
            // that we sell instead of buy as the solver just requires an order
            // selling the fee token in order to balance the fees.
            let fee_price = orderbook
                .find_optimal_transitive_order(TokenPair {
                    buy: token,
                    sell: FEE_TOKEN,
                })
                .expect("negative cycle in reduced orderbook")?
                .exchange_rate
                .price()
                .inverse();

            Some((fee_price, fee_price))
        })?;

        debug_assert!(min <= max);
        Some((min.value(), max.value()))
    }
}

#[cfg(test)]
mod tests {
    use crate::test::prelude::*;
    use crate::FEE_FACTOR;

    #[test]
    fn estimates_prices_for_tokens_in_negative_cycles() {
        //             v---0.5---\
        //  0 <-10.0-- 1          2
        //              \--1.25---^
        let pricegraph = pricegraph! {
            users {
                @0 {
                    token 0 => 1_000_000,
                }
                @1 {
                    token 1 => 1_000_000,
                }
                @2 {
                    token 2 => 1_000_000,
                }
            }
            orders {
                owner @0 buying 1 [10_000_000] selling 0 [1_000_000],

                owner @1 buying 2 [  500_000] selling 1 [1_000_000],
                owner @2 buying 1 [1_250_000] selling 2 [1_000_000],
            }
        };

        let (min, max) = pricegraph.token_price_spread(1).unwrap();
        assert_approx_eq!(min, 0.1);
        assert_approx_eq!(max, 0.1);

        let (min, max) = pricegraph.token_price_spread(2).unwrap();
        assert_approx_eq!(1.0 / min, 10.0 / (1.25 * FEE_FACTOR));
        assert_approx_eq!(1.0 / max, 10.0 * (0.5 * FEE_FACTOR));
    }

    #[test]
    fn price_is_one_for_fee_token() {
        //  0 <-0.42-- 1
        let pricegraph = pricegraph! {
            users {
                @0 {
                    token 0 => 100_000,
                }
            }
            orders {
                owner @0 buying 1 [42_000] selling 0 [100_000],
            }
        };

        assert_eq!(pricegraph.token_price_spread(0), Some((1.0, 1.0)));
    }

    #[test]
    fn estimates_prices_for_reduced_orderbooks() {
        //  0 <-0.42-- 1
        let pricegraph = pricegraph! {
            users {
                @0 {
                    token 0 => 100_000,
                }
            }
            orders {
                owner @0 buying 1 [42_000] selling 0 [100_000],
            }
        };

        let (min, max) = pricegraph.token_price_spread(1).unwrap();
        assert_approx_eq!(min, 1.0 / 0.42);
        assert_approx_eq!(max, 1.0 / 0.42);
    }
}
