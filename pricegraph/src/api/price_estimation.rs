//! Module containing limit price estimation implementation.

use crate::api::TransitiveOrder;
use crate::encoding::TokenPair;
use crate::num;
use crate::orderbook::{ExchangeRate, LimitPrice};
use crate::{Pricegraph, MIN_AMOUNT};

impl Pricegraph {
    /// Estimates an exchange rate for the specified token pair and sell volume.
    /// Returns `None` if no counter transitive orders buying the specified sell
    /// token for the specified buy token exist.
    ///
    /// Note that this price is in exchange format, that is, it is expressed as
    /// the ratio between buy and sell amounts, with implicit fees.
    pub fn estimate_limit_price(&self, pair: TokenPair, sell_amount: f64) -> Option<f64> {
        let mut orderbook = self.reduced_orderbook();

        // NOTE: This method works by searching for the "best" counter
        // transitive orders, as such we need to fill transitive orders in the
        // inverse direction: from sell token to the buy token.
        let inverse_pair = TokenPair {
            buy: pair.sell,
            sell: pair.buy,
        };

        if sell_amount == 0.0 {
            // NOTE: For a 0 volume we simulate sending an tiny epsilon of value
            // through the network without actually filling any orders.
            let mut exchange_rate = None;
            let flow = orderbook.fill_optimal_transitive_order_if(inverse_pair, |flow| {
                exchange_rate = Some(flow.exchange_rate);
                false
            });
            debug_assert!(flow.is_none());

            // NOTE: The exchange rates are for transitive orders in the inverse
            // direction, so we need to invert the exchange rate and account for
            // the fees so that the estimated exchange rate actually overlaps with
            // the last counter transtive order's exchange rate.
            return Some(exchange_rate?.inverse().price().value());
        }

        if !num::is_strictly_positive_and_finite(sell_amount) {
            return None;
        }

        let mut total_buy_volume = 0.0;
        let mut maximum_sell_amount = 0.0;
        while let Some(flow) = orderbook.fill_optimal_transitive_order_if(inverse_pair, |flow| {
            let current_exchange_rate = match LimitPrice::new(total_buy_volume / sell_amount) {
                Some(price) => price.exchange_rate(),
                None => {
                    return true;
                }
            };

            // NOTE: This implies that the added liquidity from the counter
            // transitive order at its exchange rate makes the limit price
            // worse, and we are better off just buying off all the previously
            // discovered liquidity instead of including this transitive order.
            current_exchange_rate < flow.exchange_rate.inverse()
        }) {
            // NOTE: Compute the largest order that fully overlaps all currently
            // discovered liquidity.
            let inverse_limit_price = flow.exchange_rate.inverse().price();
            total_buy_volume += flow.capacity * inverse_limit_price.value();
            maximum_sell_amount = total_buy_volume / inverse_limit_price.value();

            debug_assert!(
                {
                    let largest_order_xrate =
                        ExchangeRate::from_price_value(total_buy_volume / maximum_sell_amount)
                            .unwrap()
                            .value();
                    let error = largest_order_xrate - flow.exchange_rate.inverse().value();
                    error.abs() <= num::max_rounding_error(largest_order_xrate)
                },
                "largest order exchange rate does not match marginal exchange rate",
            );

            // NOTE: If we only have a `MIN_AMOUNT` left to sell at the
            // current exchange rate, don't try to match new transitive
            // orders since these small dust amounts will be ignored by the
            // solver anyway.
            if sell_amount - maximum_sell_amount <= MIN_AMOUNT {
                break;
            }
        }

        let price = total_buy_volume / sell_amount.max(maximum_sell_amount);
        Some(LimitPrice::new(price)?.value())
    }

    /// Returns a transitive order with a buy amount calculated such that there
    /// exists overlapping transitive orders to completely fill the specified
    /// `sell_amount`. As such, this is an estimated order that is *likely* to
    /// be matched given the **current** state of the batch.
    pub fn order_for_sell_amount(
        &self,
        pair: TokenPair,
        sell_amount: f64,
    ) -> Option<TransitiveOrder> {
        let price = self.estimate_limit_price(pair, sell_amount)?;
        Some(TransitiveOrder {
            buy: sell_amount * price,
            sell: sell_amount,
        })
    }

    /// Returns a transitive order with the largest buy and sell amounts such
    /// that its limit price **is greater than or equal to** the specified limit
    /// price and there exists overlapping transitive orders to completely fill
    /// the order. Returns `None` if no overlapping transitive orders exist at
    /// the given limit price.
    pub fn order_for_limit_price(
        &self,
        pair: TokenPair,
        limit_price: f64,
    ) -> Option<TransitiveOrder> {
        let mut orderbook = self.reduced_orderbook();

        // NOTE: This method works by searching for the "best" counter
        // transitive orders, as such we need to fill transitive orders in the
        // inverse direction and need to invert the limit price.
        let inverse_pair = TokenPair {
            buy: pair.sell,
            sell: pair.buy,
        };
        let max_xrate = LimitPrice::new(limit_price)?.exchange_rate().inverse();

        let mut total_buy_volume = 0.0;
        let mut total_sell_volume = 0.0;
        while let Some(flow) = orderbook
            .fill_optimal_transitive_order_if(inverse_pair, |flow| flow.exchange_rate <= max_xrate)
        {
            // NOTE: The transitive orders being filled are **counter orders**
            // with inverted token pairs.
            total_buy_volume += flow.capacity / flow.exchange_rate.value();
            total_sell_volume += flow.capacity;
        }

        Some(TransitiveOrder {
            buy: total_buy_volume,
            sell: total_sell_volume,
        })
    }

    /// Returns a transitive order with the largest sell amount such that there
    /// exists overlapping transitive orders to completely fill the order at the
    /// specified limit price. Returns `None` if no overlapping transitive
    /// orders exist for the given limit price.
    ///
    /// Note that this method is subtly different to
    /// `Pricegraph::order_for_limit_price` in that the limit price for the
    /// resulting order is always equal to the specified price.
    pub fn order_at_limit_price(
        &self,
        pair: TokenPair,
        limit_price: f64,
    ) -> Option<TransitiveOrder> {
        let order = self.order_for_limit_price(pair, limit_price)?;
        Some(TransitiveOrder {
            buy: order.sell * limit_price,
            sell: order.sell,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::num;
    use crate::test::prelude::*;
    use crate::FEE_FACTOR;

    #[test]
    fn estimates_correct_limit_price() {
        //    /-101.0--v
        //   /--105.0--v
        //  /---111.0--v
        // 1           2
        // ^--.0101---/
        // ^--.0105--/
        // ^--.0110-/
        let pricegraph = pricegraph! {
            users {
                @1 {
                    token 1 => 1_000_000,
                    token 2 => 100_000_000,
                }
                @2 {
                    token 1 => 1_000_000,
                    token 2 => 100_000_000,
                }
                @3 {
                    token 1 => 1_000_000,
                    token 2 => 100_000_000,
                }
            }
            orders {
                owner @1 buying 1 [1_000_000] selling 2 [99_000_000],
                owner @2 buying 1 [1_000_000] selling 2 [95_000_000],
                owner @3 buying 1 [1_000_000] selling 2 [90_000_000],

                owner @2 buying 2 [101_000_000] selling 1 [1_000_000],
                owner @1 buying 2 [105_000_000] selling 1 [1_000_000],
                owner @3 buying 2 [110_000_000] selling 1 [1_000_000],
            }
        };

        assert_approx_eq!(
            pricegraph
                .estimate_limit_price(TokenPair { buy: 2, sell: 1 }, 500_000.0)
                .unwrap(),
            99.0 / FEE_FACTOR.powi(2)
        );
        assert_approx_eq!(
            pricegraph
                .estimate_limit_price(TokenPair { buy: 1, sell: 2 }, 50_000_000.0)
                .unwrap(),
            1.0 / (101.0 * FEE_FACTOR.powi(2))
        );

        assert_approx_eq!(
            pricegraph
                .estimate_limit_price(TokenPair { buy: 2, sell: 1 }, 1_500_000.0)
                .unwrap(),
            95.0 / FEE_FACTOR.powi(2)
        );
        assert_approx_eq!(
            pricegraph
                .estimate_limit_price(TokenPair { buy: 1, sell: 2 }, 150_000_000.0)
                .unwrap(),
            1.0 / (105.0 * FEE_FACTOR.powi(2))
        );

        assert_approx_eq!(
            pricegraph
                .estimate_limit_price(TokenPair { buy: 2, sell: 1 }, 2_500_000.0)
                .unwrap(),
            90.0 / FEE_FACTOR.powi(2)
        );
        assert_approx_eq!(
            pricegraph
                .estimate_limit_price(TokenPair { buy: 1, sell: 2 }, 250_000_000.0)
                .unwrap(),
            1.0 / (110.0 * FEE_FACTOR.powi(2))
        );
    }

    #[test]
    fn estimates_best_buy_amount_for_low_liquidity() {
        //  /---1.0---v
        // 1          2
        //
        //   /--0.01--v
        //  /---1.0---v
        // 3          4
        let pricegraph = pricegraph! {
            users {
                @1 {
                    token 2 => 100_000_000,
                    token 4 => 100_000_000,
                }
            }
            orders {
                owner @1 buying 1 [100_000_000] selling 2 [100_000_000],

                owner @1 buying 3 [  1_000_000] selling 4 [1_000_000],
                owner @1 buying 3 [100_000_000] selling 4 [1_000_000],
            }
        };

        assert_approx_eq!(
            pricegraph
                .estimate_limit_price(TokenPair { buy: 2, sell: 1 }, 200_000_000.0)
                .unwrap(),
            0.5 / FEE_FACTOR
        );

        assert_approx_eq!(
            pricegraph
                .estimate_limit_price(TokenPair { buy: 4, sell: 3 }, 2_000_000.0)
                .unwrap(),
            0.5 / FEE_FACTOR
        );
        dbg!(pricegraph
            .order_for_sell_amount(TokenPair { buy: 4, sell: 3 }, 101_000_000.0 * FEE_FACTOR));
        assert_approx_eq!(
            pricegraph
                .estimate_limit_price(TokenPair { buy: 4, sell: 3 }, 101_000_000.0)
                .unwrap(),
            0.01 / FEE_FACTOR.powi(2)
        );
        assert_approx_eq!(
            pricegraph
                .estimate_limit_price(TokenPair { buy: 4, sell: 3 }, 202_000_000.0 * FEE_FACTOR)
                .unwrap(),
            0.005 / FEE_FACTOR
        );
    }

    #[test]
    fn estimated_buy_amount_monotonically_increasing() {
        //    /-25.0--v
        //   /--50.0--v
        //  /--100.0--v
        // 1          2
        let pricegraph = pricegraph! {
            users {
                @1 {
                    token 2 => 1_000_000_000,
                }
            }
            orders {
                owner @1 buying 1 [1_000_000] selling 2 [100_000_000],
                owner @1 buying 1 [2_000_000] selling 2 [100_000_000],
                owner @1 buying 1 [4_000_000] selling 2 [100_000_000],
            }
        };

        assert_approx_eq!(
            pricegraph
                .order_for_sell_amount(TokenPair { buy: 2, sell: 1 }, 500_000.0)
                .unwrap()
                .buy,
            50_000_000.0 / FEE_FACTOR.powi(2)
        );

        assert_approx_eq!(
            pricegraph
                .order_for_sell_amount(TokenPair { buy: 2, sell: 1 }, 1_000_000.0 * FEE_FACTOR)
                .unwrap()
                .buy,
            100_000_000.0 / FEE_FACTOR
        );
        assert_approx_eq!(
            pricegraph
                .order_for_sell_amount(
                    TokenPair { buy: 2, sell: 1 },
                    1_000_000.0 * FEE_FACTOR.powi(2),
                )
                .unwrap()
                .buy,
            100_000_000.0 / FEE_FACTOR
        );
        assert_approx_eq!(
            pricegraph
                .order_for_sell_amount(TokenPair { buy: 2, sell: 1 }, 1_500_000.0)
                .unwrap()
                .buy,
            100_000_000.0 / FEE_FACTOR
        );

        assert_approx_eq!(
            pricegraph
                .order_for_sell_amount(
                    TokenPair { buy: 2, sell: 1 },
                    2_000_000.0 * FEE_FACTOR.powi(1),
                )
                .unwrap()
                .buy,
            100_000_000.0 / FEE_FACTOR
        );
        assert_approx_eq!(
            pricegraph
                .order_for_sell_amount(
                    TokenPair { buy: 2, sell: 1 },
                    3_000_000.0 * FEE_FACTOR.powi(1),
                )
                .unwrap()
                .buy,
            150_000_000.0 / FEE_FACTOR
        );
        assert_approx_eq!(
            pricegraph
                .order_for_sell_amount(
                    TokenPair { buy: 2, sell: 1 },
                    4_000_000.0 * FEE_FACTOR.powi(1),
                )
                .unwrap()
                .buy,
            200_000_000.0 / FEE_FACTOR
        );

        assert_approx_eq!(
            pricegraph
                .order_for_sell_amount(
                    TokenPair { buy: 2, sell: 1 },
                    8_000_000.0 * FEE_FACTOR.powi(1),
                )
                .unwrap()
                .buy,
            200_000_000.0 / FEE_FACTOR
        );
        assert_approx_eq!(
            pricegraph
                .order_for_sell_amount(
                    TokenPair { buy: 2, sell: 1 },
                    10_000_000.0 * FEE_FACTOR.powi(1),
                )
                .unwrap()
                .buy,
            250_000_000.0 / FEE_FACTOR
        );

        assert_approx_eq!(
            pricegraph
                .order_for_sell_amount(
                    TokenPair { buy: 2, sell: 1 },
                    12_000_000.0 * FEE_FACTOR.powi(1),
                )
                .unwrap()
                .buy,
            300_000_000.0 / FEE_FACTOR
        );
        assert_approx_eq!(
            pricegraph
                .order_for_sell_amount(TokenPair { buy: 2, sell: 1 }, 100_000_000.0)
                .unwrap()
                .buy,
            300_000_000.0 / FEE_FACTOR
        );
    }

    #[test]
    fn estimates_epsilon_limit_price() {
        //   /--------1.0-------\
        //  /---1.0---v          v
        // 1          2          3
        //            ^---0.9---/
        let pricegraph = pricegraph! {
            users {
                @1 {
                    token 2 => 10_000_000,
                    token 3 => 10_000_000,
                }
                @2 {
                    token 2 => 10_000_000,
                }
            }
            orders {
                owner @1 buying 1 [10_000_000] selling 2 [10_000_000],
                owner @1 buying 1 [10_000_000] selling 3 [10_000_000],
                owner @2 buying 3 [9_000_000] selling 2 [10_000_000],
            }
        };

        assert_approx_eq!(
            pricegraph
                .estimate_limit_price(TokenPair { buy: 2, sell: 1 }, 0.0)
                .unwrap(),
            (1.0 / 0.9) / FEE_FACTOR.powi(3)
        );
    }

    #[test]
    fn order_for_limit_price_has_correct_amounts() {
        //    /-1.0---v
        //   /--2.0---v
        //  /---4.0---v
        // 1          2
        let pricegraph = pricegraph! {
            users {
                @1 {
                    token 2 => 1_000_000,
                }
                @2 {
                    token 2 => 1_000_000,
                }
                @3 {
                    token 2 => 1_000_000,
                }
            }
            orders {
                owner @1 buying 1 [1_000_000] selling 2 [1_000_000],
                owner @2 buying 1 [2_000_000] selling 2 [1_000_000],
                owner @3 buying 1 [4_000_000] selling 2 [1_000_000],
            }
        };

        let TransitiveOrder { buy, sell } = pricegraph
            // NOTE: 1 for 1.001 is not enough to match any volume because
            // fees need to be applied twice!
            .order_for_limit_price(TokenPair { buy: 2, sell: 1 }, 1.0 / FEE_FACTOR)
            .unwrap();
        assert_approx_eq!(buy, 0.0);
        assert_approx_eq!(sell, 0.0);

        let TransitiveOrder { buy, sell } = pricegraph
            .order_for_limit_price(TokenPair { buy: 2, sell: 1 }, 1.0 / FEE_FACTOR.powi(2))
            .unwrap();
        assert_approx_eq!(buy, 1_000_000.0);
        assert_approx_eq!(sell, 1_000_000.0 * FEE_FACTOR);

        let TransitiveOrder { buy, sell } = pricegraph
            .order_for_limit_price(TokenPair { buy: 2, sell: 1 }, 0.3)
            .unwrap();
        assert_approx_eq!(buy, 2_000_000.0);
        assert_approx_eq!(sell, 3_000_000.0 * FEE_FACTOR);

        let TransitiveOrder { buy, sell } = pricegraph
            .order_for_limit_price(TokenPair { buy: 2, sell: 1 }, 0.25 / FEE_FACTOR.powi(2))
            .unwrap();
        assert_approx_eq!(buy, 3_000_000.0);
        assert_approx_eq!(sell, 7_000_000.0 * FEE_FACTOR);

        let TransitiveOrder { buy, sell } = pricegraph
            .order_for_limit_price(TokenPair { buy: 2, sell: 1 }, 0.1)
            .unwrap();
        assert_approx_eq!(buy, 3_000_000.0);
        assert_approx_eq!(sell, 7_000_000.0 * FEE_FACTOR);
    }

    #[test]
    fn order_at_exact_limit_price() {
        //  /---1.0---v
        // 1          2
        let pricegraph = pricegraph! {
            users {
                @1 {
                    token 2 => 1_000_000,
                }
            }
            orders {
                owner @1 buying 1 [1_000_000] selling 2 [1_000_000],
            }
        };

        let TransitiveOrder { buy, sell } = pricegraph
            .order_for_limit_price(TokenPair { buy: 2, sell: 1 }, 0.5)
            .unwrap();
        assert_approx_eq!(buy, 1_000_000.0);
        assert_approx_eq!(sell, 1_000_000.0 * FEE_FACTOR);

        let TransitiveOrder { buy, sell } = pricegraph
            .order_at_limit_price(TokenPair { buy: 2, sell: 1 }, 0.5)
            .unwrap();
        assert_approx_eq!(buy, 500_000.0 * FEE_FACTOR);
        assert_approx_eq!(sell, 1_000_000.0 * FEE_FACTOR);
    }

    #[test]
    fn estimate_limit_price_returns_none_for_invalid_token_pairs() {
        //   /---1.0---v
        //  0          1          2 --0.5--> 4
        //  ^---1.0---/
        let pricegraph = pricegraph! {
            users {
                @1 {
                    token 1 => 1_000_000,
                }
                @2 {
                    token 0 => 1_000_000,
                }
                @3 {
                    token 4 => 1_000_000,
                }
            }
            orders {
                owner @1 buying 0 [1_000_000] selling 1 [1_000_000],
                owner @2 buying 1 [1_000_000] selling 0 [1_000_000],
                owner @3 buying 2 [1_000_000] selling 4 [1_000_000],
            }
        };

        // Token 3 is not part of the orderbook.
        assert_eq!(
            pricegraph.estimate_limit_price(TokenPair { buy: 1, sell: 3 }, 500_000.0),
            None,
        );
        // Tokens 4 and 1 are not connected.
        assert_eq!(
            pricegraph.estimate_limit_price(TokenPair { buy: 4, sell: 1 }, 500_000.0),
            None,
        );
        // Tokens 5 and 42 are out of bounds.
        assert_eq!(
            pricegraph.estimate_limit_price(TokenPair { buy: 5, sell: 1 }, 500_000.0),
            None,
        );
        assert_eq!(
            pricegraph.estimate_limit_price(TokenPair { buy: 2, sell: 42 }, 500_000.0),
            None,
        );
    }

    #[test]
    fn fuzz_calculates_rounding_errors_based_on_amounts() {
        // NOTE: Discovered by fuzzer, see
        // https://github.com/gnosis/dex-services/issues/916#issuecomment-634457245

        let pricegraph = pricegraph! {
            users {
                @1 {
                    token 1 => u128::MAX,
                }
            }
            orders {
                owner @1
                    buying  0 [ 13_294_906_614_391_990_988_372_451_468_773_477_386]
                    selling 1 [327_042_228_921_422_829_026_657_257_798_164_547_592]
                              ( 83_798_276_971_421_254_262_445_676_335_662_107_162),
            }
        };

        let TransitiveOrder { buy, sell } = pricegraph
            .order_for_limit_price(TokenPair { buy: 1, sell: 0 }, 1.0)
            .unwrap();
        assert_approx_eq!(
            buy,
            83_798_276_971_421_254_262_445_676_335_662_107_162.0,
            num::max_rounding_error_with_epsilon(buy)
        );
        assert_approx_eq!(
            sell,
            (83_798_276_971_421_254_262_445_676_335_662_107_162.0
                / 327_042_228_921_422_829_026_657_257_798_164_547_592.0)
                * 13_294_906_614_391_990_988_372_451_468_773_477_386.0
                * FEE_FACTOR,
            num::max_rounding_error_with_epsilon(sell)
        );
    }
}
