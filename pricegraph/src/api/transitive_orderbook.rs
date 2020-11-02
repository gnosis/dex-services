//! This module contains the implementation for computing a transitive orderbook
//! over a market.

use crate::api::{Market, TransitiveOrder};
use crate::encoding::TokenPairRange;
use crate::num;
use crate::orderbook::{Orderbook, OverlapError, Ring};
use crate::{Pricegraph, FEE_FACTOR};

/// A struct representing a transitive orderbook for a base and quote token.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TransitiveOrderbook {
    /// Transitive "ask" orders, i.e. transitive orders buying the quote token
    /// and selling the base token.
    pub asks: Vec<TransitiveOrder>,
    /// Transitive "bid" orders, i.e. transitive orders buying the base token
    /// and selling the quote token.
    pub bids: Vec<TransitiveOrder>,
}

impl TransitiveOrderbook {
    /// Returns an iterator with ask prices (expressed in the quote token) and
    /// corresponding volumes.
    ///
    /// Note that the prices are effective prices and include fees.
    pub fn ask_prices(&self) -> impl DoubleEndedIterator<Item = (f64, f64)> + '_ {
        self.asks
            .iter()
            .map(|order| ((order.buy / order.sell) * FEE_FACTOR, order.sell))
    }

    /// Returns an iterator with bid prices (expressed in the quote token) and
    /// corresponding volumes.
    ///
    /// Note that the prices are effective prices and include fees.
    pub fn bid_prices(&self) -> impl DoubleEndedIterator<Item = (f64, f64)> + '_ {
        self.bids
            .iter()
            .map(|order| ((order.sell / order.buy) / FEE_FACTOR, order.buy))
    }
}

impl Pricegraph {
    /// Computes a transitive orderbook for the given market.
    ///
    /// This method optionally accepts a spread that is a decimal fraction that
    /// defines the maximume transitive order price with the equation:
    /// `first_transitive_price + first_transitive_price * spread`. This means
    /// that given a spread of 0.5 (or 50%), and if the cheapest transitive
    /// order has a price of 1.2, then the maximum price will be `1.8`.
    ///
    /// The spread applies to both `asks` and `bids` transitive orders.
    ///
    /// # Panics
    ///
    /// This method panics if the spread is zero or negative.
    pub fn transitive_orderbook(
        &self,
        market: Market,
        hops: Option<usize>,
        spread: Option<f64>,
    ) -> TransitiveOrderbook {
        let mut orderbook = self.full_orderbook();

        let mut transitive_orderbook = TransitiveOrderbook::default();
        while let Some(Ring { ask, bid }) = orderbook.fill_market_ring_trade(market) {
            transitive_orderbook.asks.push(ask.as_transitive_order());
            transitive_orderbook.bids.push(bid.as_transitive_order());
        }

        // NOTE: In the case where the market `quote` and `base` token are in
        // different disconnected subgraphs, it is possible that the `base`
        // token's subgraph contains negative cycles. In order to ensure that
        // the `base` token subgraph is also reduced, fill any remaining
        // negative cycles in the inverse market. However, there should be no
        // ring trades over the inverse market (if there were, then the `quote`
        // and `base` token would be part of the same subgraph), so assert it.
        let inverse_ring = orderbook.fill_market_ring_trade(market.inverse());
        debug_assert_eq!(inverse_ring, None);

        transitive_orderbook.asks.extend(
            fill_transitive_orders(
                orderbook.clone(),
                market.ask_pair().into_range(hops),
                spread,
            )
            .expect("overlapping orders in reduced orderbook"),
        );
        transitive_orderbook.bids.extend(
            fill_transitive_orders(orderbook, market.bid_pair().into_range(hops), spread)
                .expect("overlapping orders in reduced orderbook"),
        );

        for orders in &mut [
            &mut transitive_orderbook.asks,
            &mut transitive_orderbook.bids,
        ] {
            orders.sort_unstable_by(|a, b| num::compare(a.exchange_rate(), b.exchange_rate()));
        }

        transitive_orderbook
    }

    /// Computes the best ask order for the specified market. Returns `None` if
    /// there are no remaining transitive orders after all overlapping ring
    /// trades have been removed.
    pub fn best_ask_transitive_order(&self, market: Market) -> Option<TransitiveOrder> {
        Some(
            self.reduced_orderbook()
                .find_optimal_transitive_order(market.ask_pair().into_unbounded_range())?
                .as_transitive_order(),
        )
    }

    /// Computes the best bid order for the specified market. See
    /// `Pricegraph::best_ask_order` for more details.
    pub fn best_bid_transitive_order(&self, market: Market) -> Option<TransitiveOrder> {
        Some(
            self.reduced_orderbook()
                .find_optimal_transitive_order(market.bid_pair().into_unbounded_range())?
                .as_transitive_order(),
        )
    }
}

/// Fills transitive orders along a token pair, optionally specifying a
/// maximum spread for the orders.
///
/// Returns a vector containing all the transitive orders that were filled.
///
/// Note that the spread is a decimal fraction that defines the maximum
/// transitive order exchange rate with the equation:
/// `first_transitive_xrate + first_transitive_xrate * spread`. This means
/// that given a spread of `0.5` (or 50%), and if the cheapest transitive
/// order has an exchange rate of `1.2`, then the maximum exchange rate will
/// be `1.8`.
///
/// # Panics
///
/// This method panics if the spread is zero or negative.
fn fill_transitive_orders(
    orderbook: Orderbook,
    pair_range: TokenPairRange,
    spread: Option<f64>,
) -> Result<Vec<TransitiveOrder>, OverlapError> {
    if let Some(spread) = spread {
        assert!(spread > 0.0, "invalid spread");
    }

    let mut transitive_orders = orderbook.transitive_orders(pair_range)?.peekable();
    let max_xrate = spread
        .and_then(|spread| {
            let flow = transitive_orders.peek()?;
            Some(flow.exchange_rate.value() * (1.0 + spread))
        })
        .unwrap_or(f64::INFINITY);

    Ok(transitive_orders
        .take_while(|flow| flow.exchange_rate <= max_xrate)
        .map(|flow| flow.as_transitive_order())
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::encoding::{Element, PriceFraction, TokenPair, Validity};
    use crate::test::prelude::*;
    use primitive_types::U256;

    #[test]
    fn transitive_orderbook_empty_same_token() {
        let pricegraph = Pricegraph::new(std::iter::empty());
        let orderbook = pricegraph.transitive_orderbook(Market { base: 0, quote: 0 }, None, None);
        assert!(orderbook.asks.is_empty());
        assert!(orderbook.bids.is_empty());
    }

    #[test]
    fn transitive_orderbook_simple() {
        let base: u128 = 1_000_000_000_000;
        let pricegraph = pricegraph! {
            users {
                @0 {
                    token 1 => 2 * base,
                }
            }
            orders {
                owner @0 buying 0 [2 * base] selling 1 [base],
            }
        };

        let orderbook = pricegraph.transitive_orderbook(Market { base: 0, quote: 1 }, None, None);
        assert_eq!(orderbook.asks, vec![]);
        assert_eq!(
            orderbook.bids,
            vec![TransitiveOrder {
                buy: 2.0 * base as f64,
                sell: base as f64,
            }]
        );
        let bid_price = orderbook.bid_prices().next().unwrap();
        assert_approx_eq!(bid_price.0, 0.5 / FEE_FACTOR);

        let orderbook = pricegraph.transitive_orderbook(Market { base: 1, quote: 0 }, None, None);
        assert_eq!(
            orderbook.asks,
            vec![TransitiveOrder {
                buy: 2.0 * base as f64,
                sell: base as f64,
            }]
        );
        let ask_price = orderbook.ask_prices().next().unwrap();
        assert_approx_eq!(ask_price.0, 2.0 * FEE_FACTOR);
        assert_eq!(orderbook.bids, vec![]);
    }

    #[test]
    fn transitive_orderbook_prices() {
        let transitive_orderbook = TransitiveOrderbook {
            asks: vec![
                TransitiveOrder {
                    buy: 20_000_000.0,
                    sell: 10_000_000.0,
                },
                TransitiveOrder {
                    buy: 1_500_000.0,
                    sell: 900_000.0,
                },
            ],
            bids: vec![
                TransitiveOrder {
                    buy: 1_000_000.0,
                    sell: 2_000_000.0,
                },
                TransitiveOrder {
                    buy: 500_000.0,
                    sell: 900_000.0,
                },
            ],
        };

        let ask_prices = transitive_orderbook.ask_prices().collect::<Vec<_>>();
        assert_approx_eq!(ask_prices[0].0, 2.0 * FEE_FACTOR);
        assert_approx_eq!(ask_prices[0].1, 10_000_000.0);
        assert_approx_eq!(ask_prices[1].0, (1.5 / 0.9) * FEE_FACTOR);
        assert_approx_eq!(ask_prices[1].1, 900_000.0);

        let bid_prices = transitive_orderbook.bid_prices().collect::<Vec<_>>();
        assert_approx_eq!(bid_prices[0].0, 2.0 / FEE_FACTOR);
        assert_approx_eq!(bid_prices[0].1, 1_000_000.0);
        assert_approx_eq!(bid_prices[1].0, (9.0 / 5.0) / FEE_FACTOR);
        assert_approx_eq!(bid_prices[1].1, 500_000.0);
    }

    #[test]
    fn transitive_orderbook_hops() {
        //  0 --2.0-> 1 --1.0-> 5 <--\
        //  |         |         |    |
        // 1.0       2.0       0.5  1.0
        //  ∨         ∨         ∨    |
        //  2         3         6----/
        //  |         ∧         |
        // 1.0       2.0        |
        //  |         |         |
        //  \-------> 4 <------0.5
        let pricegraph = pricegraph! {
            users {
                @1 {
                    token 1 => 2_000_000,
                }
                @2 {
                    token 2 => 1_000_000,
                }
                @3 {
                    token 3 => 2_000_000,
                }
                @4 {
                    token 5 => 1_000_000,
                }
                @5 {
                    token 4 => 1_000_000,
                }
                @6 {
                    token 3 => 2_000_000,
                }
                @7 {
                    token 6 => 500_000,
                }
                @8 {
                    token 5 => 10_000_000,
                }
                @9 {
                    token 4 => 1_000_000,
                }
            }
            orders {
                owner @1 buying 0 [1_000_000] selling 1 [2_000_000],
                owner @2 buying 0 [1_000_000] selling 2 [1_000_000],
                owner @3 buying 1 [1_000_000] selling 3 [2_000_000],
                owner @4 buying 1 [1_000_000] selling 5 [1_000_000],
                owner @5 buying 2 [1_000_000] selling 4 [1_000_000],
                owner @6 buying 4 [1_000_000] selling 3 [2_000_000],
                owner @7 buying 5 [1_000_000] selling 6 [500_000],
                owner @8 buying 6 [10_000_000] selling 5 [10_000_000],
                owner @9 buying 6 [1_000_000] selling 4 [1_000_000],
            }
        };
        let market = Market { base: 0, quote: 3 };

        // All transitive orders need at most two hops to be computed.
        // There is a small negative loop between the tokens 5 and 6
        // which makes the path 0 -> 1 -> 5 -> 6 -> 4 -> 3 disappear.

        let TransitiveOrderbook { bids, asks } =
            pricegraph.transitive_orderbook(market, Some(10), None);
        assert_eq!(asks.len(), 0);
        assert_eq!(bids.len(), 2);
        assert_approx_eq!(bids[0].buy, 500_000.0 * FEE_FACTOR);
        assert_approx_eq!(bids[0].sell, 2_000_000.0);
        assert_approx_eq!(bids[1].buy, 1_000_000.0);
        assert_approx_eq!(bids[1].sell, 2_000_000.0 / FEE_FACTOR.powi(2));

        let TransitiveOrderbook { bids, asks } =
            pricegraph.transitive_orderbook(market, Some(2), None);
        assert_eq!(asks.len(), 0);
        assert_eq!(bids.len(), 1);
        assert_approx_eq!(bids[0].buy, 500_000.0 * FEE_FACTOR);
        assert_approx_eq!(bids[0].sell, 2_000_000.0);

        let TransitiveOrderbook { bids, asks } =
            pricegraph.transitive_orderbook(market, Some(1), None);
        assert_eq!(asks.len(), 0);
        assert_eq!(bids.len(), 0);
    }

    #[test]
    fn detects_overlapping_transitive_orders() {
        // 0 --1.0--> 1 --0.5--> 2 --1.0--> 3 --1.0--> 4
        //            ^---------1.0--------/^---0.5---/
        let pricegraph = pricegraph! {
            users {
                @1 {
                    token 1 => 1_000_000,
                }
                @2 {
                    token 2 => 2_000_000,
                }
                @3 {
                    token 3 => 1_000_000,
                }

                @4 {
                    token 4 => 1_000_000,
                }
                @5 {
                    token 3 => 2_000_000,
                }
            }
            orders {
                owner @1 buying 0 [1_000_000] selling 1 [1_000_000],
                owner @1 buying 3 [1_000_000] selling 1 [1_000_000],
                owner @2 buying 1 [1_000_000] selling 2 [2_000_000],
                owner @3 buying 2 [1_000_000] selling 3 [1_000_000] (500_000),

                owner @4 buying 3 [1_000_000] selling 4 [1_000_000],
                owner @5 buying 4 [1_000_000] selling 3 [2_000_000],
            }
        };

        let transitive_orderbook =
            pricegraph.transitive_orderbook(Market { base: 1, quote: 2 }, None, None);

        // Transitive order `2 -> 3 -> 1` buying 2 selling 1
        assert_eq!(transitive_orderbook.asks.len(), 1);
        assert_approx_eq!(transitive_orderbook.asks[0].buy, 500_000.0);
        assert_approx_eq!(transitive_orderbook.asks[0].sell, 500_000.0 / FEE_FACTOR);

        // Transitive order `1 -> 2` buying 1 selling 2
        assert_eq!(transitive_orderbook.bids.len(), 1);
        assert_approx_eq!(transitive_orderbook.bids[0].buy, 1_000_000.0);
        assert_approx_eq!(transitive_orderbook.bids[0].sell, 2_000_000.0);
    }

    #[test]
    fn includes_transitive_order_only_once() {
        // /---0.5---v
        // 0         1
        // ^---1.0---/
        // ^---1.5--/
        let pricegraph = pricegraph! {
            users {
                @1 {
                    token 1 => 100_000_000,
                }
                @2 {
                    token 0 => 1_000_000,
                }
                @3 {
                    token 0 => 1_000_000,
                }
            }
            orders {
                owner @1 buying 0 [50_000_000] selling 1 [100_000_000],
                owner @2 buying 1 [1_000_000] selling 0 [1_000_000],
                owner @3 buying 1 [1_500_000] selling 0 [1_000_000],
            }
        };

        let transitive_orderbook =
            pricegraph.transitive_orderbook(Market { base: 0, quote: 1 }, None, None);

        // Transitive orders `1 -> 0` buying 1 selling 0
        assert_eq!(transitive_orderbook.asks.len(), 2);
        assert_approx_eq!(transitive_orderbook.asks[0].buy, 1_000_000.0);
        assert_approx_eq!(transitive_orderbook.asks[0].sell, 1_000_000.0);
        assert_approx_eq!(transitive_orderbook.asks[1].buy, 1_500_000.0);
        assert_approx_eq!(transitive_orderbook.asks[1].sell, 1_000_000.0);

        // Transitive order `0 -> 1` buying 0 selling 1
        assert_eq!(transitive_orderbook.bids.len(), 1);
        assert_approx_eq!(transitive_orderbook.bids[0].buy, 50_000_000.0);
        assert_approx_eq!(transitive_orderbook.bids[0].sell, 100_000_000.0);
    }

    #[test]
    fn fills_transitive_orders_with_maximum_spread() {
        //    /--1.0--v
        //   /        v---2.0--\
        //  /---4.0---v         \
        // 1          2          3
        //  \                    ^
        //   \--------1.0-------/
        let pricegraph = pricegraph! {
            users {
                @1 {
                    token 2 => 1_000_000,
                    token 3 => 1_000_000,
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
                owner @2 buying 3 [2_000_000] selling 2 [1_000_000],
                owner @3 buying 1 [4_000_000] selling 2 [1_000_000],

                owner @1 buying 1 [1_000_000] selling 3 [1_000_000],
            }
        };
        let market = Market { base: 1, quote: 2 };

        let TransitiveOrderbook { bids, .. } =
            pricegraph.transitive_orderbook(market, None, Some(0.5));
        assert_eq!(bids.len(), 1);
        assert_approx_eq!(bids[0].buy, 1_000_000.0);
        assert_approx_eq!(bids[0].sell, 1_000_000.0);

        let TransitiveOrderbook { bids, .. } =
            pricegraph.transitive_orderbook(market, None, Some(1.0));
        assert_eq!(bids.len(), 1);

        let TransitiveOrderbook { bids, .. } =
            pricegraph.transitive_orderbook(market, None, Some((2.0 * FEE_FACTOR) - 1.0));
        assert_eq!(bids.len(), 2);
        assert_approx_eq!(bids[1].buy, 1_000_000.0);
        assert_approx_eq!(bids[1].sell, 500_000.0 / FEE_FACTOR);

        let TransitiveOrderbook { bids, .. } =
            pricegraph.transitive_orderbook(market, None, Some(3.0));
        assert_eq!(bids.len(), 3);
        assert_approx_eq!(bids[2].buy, 4_000_000.0);
        assert_approx_eq!(bids[2].sell, 1_000_000.0);
    }

    #[test]
    fn fills_all_transitive_orders_without_maximum_spread() {
        //    /--1.0--v
        //   /        v---2.0--\
        //  /---4.0---v         \
        // 1          2          3
        //  \                    ^
        //   \--------1.0-------/
        let pricegraph = pricegraph! {
            users {
                @1 {
                    token 2 => 1_000_000,
                    token 3 => 1_000_000,
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
                owner @2 buying 3 [2_000_000] selling 2 [1_000_000],
                owner @3 buying 1 [4_000_000] selling 2 [1_000_000],

                owner @1 buying 1 [1_000_000] selling 3 [1_000_000],
            }
        };
        let market = Market { base: 1, quote: 2 };

        let TransitiveOrderbook { bids, .. } = pricegraph.transitive_orderbook(market, None, None);
        assert_eq!(bids.len(), 3);

        assert_approx_eq!(bids[0].buy, 1_000_000.0);
        assert_approx_eq!(bids[0].sell, 1_000_000.0);

        assert_approx_eq!(bids[1].buy, 1_000_000.0);
        assert_approx_eq!(bids[1].sell, 500_000.0 / FEE_FACTOR);

        assert_approx_eq!(bids[2].buy, 4_000_000.0);
        assert_approx_eq!(bids[2].sell, 1_000_000.0);
    }

    #[test]
    fn fills_all_transitive_orders_with_hops_and_no_spread() {
        //    /--1.0--v
        //   /        v---2.0--\
        //  /---4.0---v         \
        // 1          2          3
        //  \                    ^
        //   \--------1.0-------/
        let pricegraph = pricegraph! {
            users {
                @1 {
                    token 2 => 1_000_000,
                    token 3 => 1_000_000,
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
                owner @2 buying 3 [2_000_000] selling 2 [1_000_000],
                owner @3 buying 1 [4_000_000] selling 2 [1_000_000],

                owner @1 buying 1 [1_000_000] selling 3 [1_000_000],
            }
        };
        let market = Market { base: 1, quote: 2 };

        let TransitiveOrderbook { bids, .. } =
            pricegraph.transitive_orderbook(market, Some(1), None);
        assert_eq!(bids.len(), 2);

        assert_approx_eq!(bids[0].buy, 1_000_000.0);
        assert_approx_eq!(bids[0].sell, 1_000_000.0);

        assert_approx_eq!(bids[1].buy, 4_000_000.0);
        assert_approx_eq!(bids[1].sell, 1_000_000.0);
    }

    #[test]
    fn fills_all_transitive_orders_with_hops_and_spread() {
        //    /--1.0--v
        //   /        v---2.0--\
        //  /---4.0---v         \
        // 1          2          3
        //  \                    ^
        //   \--------1.0-------/
        let pricegraph = pricegraph! {
            users {
                @1 {
                    token 2 => 1_000_000,
                    token 3 => 1_000_000,
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
                owner @2 buying 3 [2_000_000] selling 2 [1_000_000],
                owner @3 buying 1 [4_000_000] selling 2 [1_000_000],

                owner @1 buying 1 [1_000_000] selling 3 [1_000_000],
            }
        };
        let market = Market { base: 1, quote: 2 };

        let TransitiveOrderbook { bids, .. } =
            pricegraph.transitive_orderbook(market, Some(1), Some(3.0));
        assert_eq!(bids.len(), 2);
        assert_approx_eq!(bids[1].buy, 4_000_000.0);
        assert_approx_eq!(bids[1].sell, 1_000_000.0);
    }

    #[test]
    fn transitive_orderbook_reduces_remaining_negative_cycles_in_inverse_market() {
        //   /--------------1.0------------\
        //  /---0.5---v                     v
        // 0          1          2          3
        // ^---0.5---/
        let pricegraph = pricegraph! {
            users {
                @0 {
                    token 0 => 1_000_000,
                }
                @1 {
                    token 0 => 1_000_000,
                }
                @2 {
                    token 1 => 1_000_000,
                }
            }
            orders {
                owner @0 buying 3 [1_000_000] selling 0 [1_000_000],
                owner @1 buying 1 [500_000] selling 0 [1_000_000],
                owner @2 buying 0 [500_000] selling 1 [1_000_000],
            }
        };
        let transitive_orderbook =
            pricegraph.transitive_orderbook(Market { base: 3, quote: 2 }, None, None);
        assert!(transitive_orderbook.asks.is_empty() && transitive_orderbook.bids.is_empty());
    }

    #[test]
    fn transitive_orderbook_with_unlimited_order_and_large_balance_doesnt_oom() {
        let pricegraph = Pricegraph::new(vec![Element {
            user: Default::default(),
            balance: U256::MAX,
            pair: TokenPair { buy: 0, sell: 1 },
            valid: Validity { from: 0, to: 0 },
            price: PriceFraction {
                numerator: u128::MAX,
                denominator: 10u128.pow(18),
            },
            remaining_sell_amount: 10u128.pow(18),
            id: 0,
        }]);

        let transitive_orderbook =
            pricegraph.transitive_orderbook(Market { base: 1, quote: 0 }, None, None);

        // NOTE: Previously, this orderbook would cause an OOM error since it
        // would only be able to reduce a maximum of `u128::MAX` per iteration.
        // Since this has a balance of `U256::MAX`, this would mean that it
        // would require `2^128` iterations, creating that many transitive
        // orders, which clearly does not fit in memory.
        assert_eq!(transitive_orderbook.asks.len(), 1);
        assert!(transitive_orderbook.bids.is_empty());
    }
}
