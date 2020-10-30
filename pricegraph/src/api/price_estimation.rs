//! Module containing limit price estimation implementation.

use crate::api::TransitiveOrder;
use crate::encoding::TokenPairRange;
use crate::num;
use crate::orderbook::{ExchangeRate, LimitPrice};
use crate::Pricegraph;

impl Pricegraph {
    /// Estimates an exchange rate for the specified token pair and sell volume.
    /// Returns `None` if no counter transitive orders buying the specified sell
    /// token for the specified buy token exist, or if the trade would end up
    /// being a dust trade.
    ///
    /// Note that this price is in exchange format, that is, it is expressed as
    /// the ratio between buy and sell amounts, with implicit fees.
    pub fn estimate_limit_price(&self, pair_range: TokenPairRange, max_sell_amount: f64) -> Option<f64> {
        if !num::is_strictly_positive_and_finite(max_sell_amount)
            || num::is_dust_amount(max_sell_amount as u128)
        {
            return None;
        }

        // NOTE: This method works by searching for the "best" counter
        // transitive orders, as such we need to fill transitive orders in the
        // inverse direction: from sell token to the buy token.
        let inverse_pair_range = pair_range.inverse();

        // NOTE: Iteratively compute the how much cumulative buy volume is
        // available at successively "worse" exchange rates until all the
        // specified sell amount can be used to buy the available liquidity at
        // the marginal exchange rate.
        let mut cumulative_buy_volume = 0.0;
        let mut cumulative_sell_volume = 0.0;
        for flow in self
            .reduced_orderbook()
            .significant_transitive_orders(inverse_pair_range)
        {
            // NOTE: This implies that the added liquidity from the counter
            // transitive order at its exchange rate makes the estimated
            // exchange rate worse, and we are better off just buying off all
            // the previously discovered liquidity instead of including new
            // liquidity from this transitive order.
            if matches!(
                ExchangeRate::new(cumulative_buy_volume / max_sell_amount),
                Some(current_exchange_rate)
                    if current_exchange_rate >= flow.exchange_rate.inverse()
            ) {
                break;
            }

            cumulative_buy_volume += flow.capacity / flow.exchange_rate.value();
            cumulative_sell_volume = cumulative_buy_volume * flow.exchange_rate.value();

            // NOTE: We've found enough liquidity to completely sell the
            // specified sell volume, so we can stop searching.
            if cumulative_sell_volume >= max_sell_amount {
                break;
            }
        }

        let total_sell_volume = max_sell_amount.max(cumulative_sell_volume);
        let price = ExchangeRate::new(cumulative_buy_volume / total_sell_volume)?
            .price()
            .value();

        // NOTE: While technically an order with a dust buy amount is not a dust
        // order (since the solver may chose to use an executed buy amount
        // greater than the dust amount), the pricegraph has determined that
        // there are no overlapping order with a sufficiently low price, so
        // there is no price that can be used for the specified sell amount and
        // be overlapping with existing orders such that an executed buy amount
        // could be found greater than the dust amount.
        let min_buy_amount = max_sell_amount * price;
        if num::is_dust_amount(min_buy_amount as u128) {
            return None;
        }

        Some(price)
    }

    /// Returns a transitive order with a buy amount calculated such that there
    /// exists overlapping transitive orders to completely fill the specified
    /// `sell_amount`. As such, this is an estimated order that is *likely* to
    /// be matched given the **current** state of the batch.
    pub fn order_for_sell_amount(
        &self,
        pair_range: TokenPairRange,
        sell_amount: f64,
    ) -> Option<TransitiveOrder> {
        let price = self.estimate_limit_price(pair_range, sell_amount)?;
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
        pair_range: TokenPairRange,
        limit_price: f64,
    ) -> Option<TransitiveOrder> {
        // NOTE: This method works by searching for the "best" counter
        // transitive orders, as such we need to fill transitive orders in the
        // inverse direction and need to invert the limit price.
        let inverse_pair_range = pair_range.inverse();
        let max_xrate = LimitPrice::new(limit_price)?.exchange_rate().inverse();

        let (total_buy_volume, total_sell_volume) = self
            .reduced_orderbook()
            .significant_transitive_orders(inverse_pair_range)
            .take_while(|flow| flow.exchange_rate <= max_xrate)
            .fold((0.0, 0.0), |(total_buy_volume, total_sell_volume), flow| {
                (
                    total_buy_volume + flow.capacity / flow.exchange_rate.value(),
                    total_sell_volume + flow.capacity,
                )
            });

        if total_buy_volume == 0.0 || total_sell_volume == 0.0 {
            None
        } else {
            Some(TransitiveOrder {
                buy: total_buy_volume,
                sell: total_sell_volume,
            })
        }
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
        pair_range: TokenPairRange,
        limit_price: f64,
    ) -> Option<TransitiveOrder> {
        let order = self.order_for_limit_price(pair_range, limit_price)?;
        Some(TransitiveOrder {
            buy: order.sell * limit_price,
            sell: order.sell,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::encoding::TokenPair;
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
                .estimate_limit_price(TokenPair { buy: 2, sell: 1 }.into_unbounded_range(), 500_000.0)
                .unwrap(),
            99.0 / FEE_FACTOR.powi(2)
        );
        assert_approx_eq!(
            pricegraph
                .estimate_limit_price(TokenPair { buy: 1, sell: 2 }.into_unbounded_range(), 50_000_000.0)
                .unwrap(),
            1.0 / (101.0 * FEE_FACTOR.powi(2))
        );

        assert_approx_eq!(
            pricegraph
                .estimate_limit_price(TokenPair { buy: 2, sell: 1 }.into_unbounded_range(), 1_500_000.0)
                .unwrap(),
            95.0 / FEE_FACTOR.powi(2)
        );
        assert_approx_eq!(
            pricegraph
                .estimate_limit_price(TokenPair { buy: 1, sell: 2 }.into_unbounded_range(), 150_000_000.0)
                .unwrap(),
            1.0 / (105.0 * FEE_FACTOR.powi(2))
        );

        assert_approx_eq!(
            pricegraph
                .estimate_limit_price(TokenPair { buy: 2, sell: 1 }.into_unbounded_range(), 2_500_000.0)
                .unwrap(),
            90.0 / FEE_FACTOR.powi(2)
        );
        assert_approx_eq!(
            pricegraph
                .estimate_limit_price(TokenPair { buy: 1, sell: 2 }.into_unbounded_range(), 250_000_000.0)
                .unwrap(),
            1.0 / (110.0 * FEE_FACTOR.powi(2))
        );
    }

    #[test]
    fn estimates_best_buy_amount_for_low_liquidity() {
        //  /---1.0---v
        // 1          2
        //
        //   /--100.0-v
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

        // NOTE: If we buy all available token 2 from the 1->2 order, then we
        // would receive at most `100_000_000 / FEE_FACTOR` of token 2.
        assert_approx_eq!(
            pricegraph
                .estimate_limit_price(TokenPair { buy: 2, sell: 1 }.into_unbounded_range(), 200_000_000.0)
                .unwrap(),
            0.5 / FEE_FACTOR
        );

        // NOTE: If we buy all available token 4 from the first 3->4 order, then
        // we would receive at most `1_000_000 / FEE_FACTOR` of token 4. Note
        // that this yields a better price than buying token 4 from the second
        // order at `0.01` price.
        assert_approx_eq!(
            pricegraph
                .estimate_limit_price(TokenPair { buy: 4, sell: 3 }.into_unbounded_range(), 2_000_000.0)
                .unwrap(),
            0.5 / FEE_FACTOR
        );

        // NOTE: At this point, it is worth it to start using the liquidity
        // from the second 3->4 order at the `100:1` limit price.
        assert_approx_eq!(
            pricegraph
                .estimate_limit_price(TokenPair { buy: 4, sell: 3 }.into_unbounded_range(), 101_000_000.0 * FEE_FACTOR)
                .unwrap(),
            0.01 / FEE_FACTOR.powi(2)
        );
        assert_approx_eq!(
            pricegraph
                .estimate_limit_price(TokenPair { buy: 4, sell: 3 }.into_unbounded_range(), 200_000_000.0 * FEE_FACTOR)
                .unwrap(),
            0.01 / FEE_FACTOR.powi(2)
        );

        // NOTE: If we buy all available token 4 then would receive at most
        // `2_000_000 / FEE_FACTOR`.
        assert_approx_eq!(
            pricegraph
                .estimate_limit_price(TokenPair { buy: 4, sell: 3 }.into_unbounded_range(), 400_000_000.0)
                .unwrap(),
            0.005 / FEE_FACTOR
        );
    }

    #[test]
    fn estimated_buy_amount_monotonically_increasing() {
        //    /-0.04--v
        //   /--0.02--v
        //  /---0.01--v
        // 1           2
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

        // NOTE: Partially use liquidity provided by the first order.
        assert_approx_eq!(
            pricegraph
                .order_for_sell_amount(TokenPair { buy: 2, sell: 1 }.into_unbounded_range(), 500_000.0)
                .unwrap()
                .buy,
            50_000_000.0 / FEE_FACTOR.powi(2)
        );

        // NOTE: Fully use liquidity provided by the first order. Note that it
        // can send at most `100_000_000 / FEE_FACTOR` because of fees.
        assert_approx_eq!(
            pricegraph
                .order_for_sell_amount(TokenPair { buy: 2, sell: 1 }.into_unbounded_range(), 1_000_000.0 * FEE_FACTOR)
                .unwrap()
                .buy,
            100_000_000.0 / FEE_FACTOR
        );

        // NOTE: For the next ~1_000_000 sell amount, we continue to fully use
        // liquidity from the first order, and not include the second order
        // because its limit price is too high.
        assert_approx_eq!(
            pricegraph
                .order_for_sell_amount(
                    TokenPair { buy: 2, sell: 1 }.into_unbounded_range(),
                    1_000_000.0 * FEE_FACTOR + 1.0,
                )
                .unwrap()
                .buy,
            100_000_000.0 / FEE_FACTOR
        );
        assert_approx_eq!(
            pricegraph
                .order_for_sell_amount(TokenPair { buy: 2, sell: 1 }.into_unbounded_range(), 1_500_000.0)
                .unwrap()
                .buy,
            100_000_000.0 / FEE_FACTOR
        );
        assert_approx_eq!(
            pricegraph
                .order_for_sell_amount(TokenPair { buy: 2, sell: 1 }.into_unbounded_range(), 2_000_000.0 * FEE_FACTOR)
                .unwrap()
                .buy,
            100_000_000.0 / FEE_FACTOR
        );

        // NOTE: Partially use liquidity from the first and second orders.
        assert_approx_eq!(
            pricegraph
                .order_for_sell_amount(TokenPair { buy: 2, sell: 1 }.into_unbounded_range(), 3_000_000.0 * FEE_FACTOR)
                .unwrap()
                .buy,
            150_000_000.0 / FEE_FACTOR
        );

        // NOTE: Fully use liquidity from the first and second orders for the
        // next ~4_000_000 amount sold (until the limit price overlaps with the
        // third order's limit price).
        assert_approx_eq!(
            pricegraph
                .order_for_sell_amount(TokenPair { buy: 2, sell: 1 }.into_unbounded_range(), 4_000_000.0 * FEE_FACTOR)
                .unwrap()
                .buy,
            200_000_000.0 / FEE_FACTOR
        );
        assert_approx_eq!(
            pricegraph
                .order_for_sell_amount(TokenPair { buy: 2, sell: 1 }.into_unbounded_range(), 8_000_000.0 * FEE_FACTOR)
                .unwrap()
                .buy,
            200_000_000.0 / FEE_FACTOR
        );

        // NOTE: Partially use liquidity from all orders.
        assert_approx_eq!(
            pricegraph
                .order_for_sell_amount(TokenPair { buy: 2, sell: 1 }.into_unbounded_range(), 10_000_000.0 * FEE_FACTOR)
                .unwrap()
                .buy,
            250_000_000.0 / FEE_FACTOR
        );

        // NOTE: Exactly use all liquidity from all orders.
        assert_approx_eq!(
            pricegraph
                .order_for_sell_amount(TokenPair { buy: 2, sell: 1 }.into_unbounded_range(), 12_000_000.0 * FEE_FACTOR)
                .unwrap()
                .buy,
            300_000_000.0 / FEE_FACTOR
        );

        // NOTE: Completely use liquidity from all orders, note that even as the
        // sell amount increases, the total buy amount does not and is capped at
        // the total liquidity in the orderbook.
        assert_approx_eq!(
            pricegraph
                .order_for_sell_amount(TokenPair { buy: 2, sell: 1 }.into_unbounded_range(), 100_000_000.0)
                .unwrap()
                .buy,
            300_000_000.0 / FEE_FACTOR
        );
    }

    #[test]
    fn estimate_returns_none_on_invalid_sell_amounts() {
        // 1 ---1.0---> 2
        let pricegraph = pricegraph! {
            users {
                @1 {
                    token 2 => 10_000_000,
                }
            }
            orders {
                owner @1 buying 1 [10_000_000] selling 2 [10_000_000],
            }
        };
        let pair_range = TokenPair { buy: 2, sell: 1 }.into_unbounded_range();

        // NOTE: Make sure that the pricegraph instance returns estimates for
        // valid amounts for the token pair.
        assert!(pricegraph.estimate_limit_price(pair_range, 1_000_000.0).is_some());

        for invalid_amount in &[-42.0, -0.0, 0.0, f64::INFINITY, f64::NEG_INFINITY, f64::NAN] {
            assert_eq!(pricegraph.estimate_limit_price(pair_range, *invalid_amount), None);
        }
    }

    #[test]
    fn order_for_limit_returns_none() {
        let pricegraph = Pricegraph::new(std::iter::empty());
        let result = pricegraph.order_for_limit_price(TokenPair { buy: 0, sell: 1 }.into_unbounded_range(), 1.0);
        assert_eq!(result, None);
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

        let order = pricegraph
            // NOTE: 1 for 1.001 is not enough to match any volume because
            // fees need to be applied twice!
            .order_for_limit_price(TokenPair { buy: 2, sell: 1 }.into_unbounded_range(), 1.0 / FEE_FACTOR);
        assert_eq!(order, None);

        let TransitiveOrder { buy, sell } = pricegraph
            .order_for_limit_price(TokenPair { buy: 2, sell: 1 }.into_unbounded_range(), 1.0 / FEE_FACTOR.powi(2))
            .unwrap();
        assert_approx_eq!(buy, 1_000_000.0);
        assert_approx_eq!(sell, 1_000_000.0 * FEE_FACTOR);

        let TransitiveOrder { buy, sell } = pricegraph
            .order_for_limit_price(TokenPair { buy: 2, sell: 1 }.into_unbounded_range(), 0.3)
            .unwrap();
        assert_approx_eq!(buy, 2_000_000.0);
        assert_approx_eq!(sell, 3_000_000.0 * FEE_FACTOR);

        let TransitiveOrder { buy, sell } = pricegraph
            .order_for_limit_price(TokenPair { buy: 2, sell: 1 }.into_unbounded_range(), 0.25 / FEE_FACTOR.powi(2))
            .unwrap();
        assert_approx_eq!(buy, 3_000_000.0);
        assert_approx_eq!(sell, 7_000_000.0 * FEE_FACTOR);

        let TransitiveOrder { buy, sell } = pricegraph
            .order_for_limit_price(TokenPair { buy: 2, sell: 1 }.into_unbounded_range(), 0.1)
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
            .order_for_limit_price(TokenPair { buy: 2, sell: 1 }.into_unbounded_range(), 0.5)
            .unwrap();
        assert_approx_eq!(buy, 1_000_000.0);
        assert_approx_eq!(sell, 1_000_000.0 * FEE_FACTOR);

        let TransitiveOrder { buy, sell } = pricegraph
            .order_at_limit_price(TokenPair { buy: 2, sell: 1 }.into_unbounded_range(), 0.5)
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
            pricegraph.estimate_limit_price(TokenPair { buy: 1, sell: 3 }.into_unbounded_range(), 500_000.0),
            None,
        );
        // Tokens 4 and 1 are not connected.
        assert_eq!(
            pricegraph.estimate_limit_price(TokenPair { buy: 4, sell: 1 }.into_unbounded_range(), 500_000.0),
            None,
        );
        // Tokens 5 and 42 are out of bounds.
        assert_eq!(
            pricegraph.estimate_limit_price(TokenPair { buy: 5, sell: 1 }.into_unbounded_range(), 500_000.0),
            None,
        );
        assert_eq!(
            pricegraph.estimate_limit_price(TokenPair { buy: 2, sell: 42 }.into_unbounded_range(), 500_000.0),
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
            .order_for_limit_price(TokenPair { buy: 1, sell: 0 }.into_unbounded_range(), 1.0)
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

    #[test]
    fn skips_dust_trades() {
        //  0 --10.0-> 1 --0.01-> 2 --0.1--> 3
        //   \
        //    \-0.1--> 4
        //
        //             5 --1.0--> 6 -100.0-> 7
        let pricegraph = pricegraph! {
            users {
                @1 {
                    token 1 => 10_000_000,
                    token 2 => 10_000_000,
                    token 3 => 10_000_000,
                }
                @2 {
                    token 4 => 10_000_000,
                }
                @3 {
                    token 6 => 10_000_000,
                    token 7 => 10_000_000,
                }
            }
            orders {
                owner @1 buying 0 [100_010] selling 1 [   10_001],
                owner @1 buying 1 [ 10_001] selling 2 [  100_010],
                owner @1 buying 2 [ 10_001] selling 3 [1_000_100],

                owner @2 buying 0 [9000] selling 4 [90_000],

                owner @3 buying 5 [   10_001] selling 6 [10_001],
                owner @3 buying 6 [1_000_100] selling 7 [10_001],
            }
        };

        // NOTE: There should be no valid transitive orders for the following
        // token pairs, since the transitive orders require trades with amounts
        // below the minimum:

        // NOTE: This would trade ~10_000 -> ~1_000 -> ~100_000 -> ~1_000_000
        assert_eq!(
            pricegraph.estimate_limit_price(TokenPair { buy: 0, sell: 3 }.into_unbounded_range(), 10_001.0),
            None,
        );

        // NOTE: This would trade ~1_000 -> ~100_000 -> ~1_000_000
        assert_eq!(
            pricegraph.estimate_limit_price(TokenPair { buy: 1, sell: 3 }.into_unbounded_range(), 10_001.0),
            None,
        );

        // NOTE: This would trade ~9_000 -> ~90_000
        assert_eq!(
            pricegraph.estimate_limit_price(TokenPair { buy: 0, sell: 4 }.into_unbounded_range(), 10_001.0),
            None,
        );

        // NOTE: This would trade ~10_000 -> ~10_000 -> ~100
        assert_eq!(
            pricegraph.estimate_limit_price(TokenPair { buy: 5, sell: 7 }.into_unbounded_range(), 10_001.0),
            None,
        );
    }

    #[test]
    fn estimate_returns_none_for_dust_sell_amounts() {
        // 1 ---0.1---> 2 ---2.0---> 3
        let pricegraph = pricegraph! {
            users {
                @1 {
                    token 2 => 10_000_000,
                }
                @2 {
                    token 3 => 10_000_000,
                }
            }
            orders {
                owner @1 buying 1 [ 1_000_000] selling 2 [10_000_000],
                owner @2 buying 2 [20_000_000] selling 3 [10_000_000],
            }
        };
        let pair_range = TokenPair { buy: 2, sell: 1 }.into_unbounded_range();

        // NOTE: Check that dust maximum sell amounts return `None`
        assert!(pricegraph.estimate_limit_price(pair_range, 10_000.0).is_some());
        assert!(pricegraph.estimate_limit_price(pair_range, 9_999.0).is_none());
    }

    #[test]
    fn estimate_returns_none_for_dust_buy_amounts() {
        // 1 ---2.0---> 2
        let pricegraph = pricegraph! {
            users {
                @1 {
                    token 2 => 10_000_000,
                }
            }
            orders {
                owner @1 buying 1 [20_000_000] selling 2 [10_000_000],
            }
        };
        let pair_range = TokenPair { buy: 2, sell: 1 }.into_unbounded_range();

        // NOTE: Check that if we try to sell less that ~20K of token 1, the
        // price estimate returns `None`, this is because there is no possible
        // executed buy amount that respects the limit price of the user @1's
        // order while simultaneously being greater than the dust amount given
        // the specified maximum sell amount.
        assert!(pricegraph
            .estimate_limit_price(pair_range, 20_000.0 * FEE_FACTOR.powi(2) + 1.0)
            .is_some());
        assert!(pricegraph.estimate_limit_price(pair_range, 15_000.0).is_none());
    }
}
