//! This module contains the implementation for computing a transitive orderbook
//! over a market.

use crate::api::{Market, TransitiveOrder};
use crate::num;
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
    pub fn transitive_orderbook(&self, market: Market, spread: Option<f64>) -> TransitiveOrderbook {
        let mut orderbook = self.full_orderbook();

        let mut transitive_orderbook = orderbook.reduce_overlapping_transitive_orderbook(market);
        transitive_orderbook.asks.extend(
            orderbook
                .clone()
                .fill_transitive_orders(market.ask_pair(), spread)
                .expect("overlapping orders in reduced orderbook"),
        );
        transitive_orderbook.bids.extend(
            orderbook
                .fill_transitive_orders(market.bid_pair(), spread)
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test::prelude::*;

    #[test]
    fn transitive_orderbook_empty_same_token() {
        let pricegraph = Pricegraph::new(std::iter::empty());
        let orderbook = pricegraph.transitive_orderbook(Market { base: 0, quote: 0 }, None);
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

        let orderbook = pricegraph.transitive_orderbook(Market { base: 0, quote: 1 }, None);
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

        let orderbook = pricegraph.transitive_orderbook(Market { base: 1, quote: 0 }, None);
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
            pricegraph.transitive_orderbook(Market { base: 3, quote: 2 }, None);
        assert!(transitive_orderbook.asks.is_empty() && transitive_orderbook.bids.is_empty());
    }
}
