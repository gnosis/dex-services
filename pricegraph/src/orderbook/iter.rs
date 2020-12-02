//! Module containing transitive order iterator for an orderbook.

use crate::{
    encoding::TokenPairRange,
    graph::path::Path,
    orderbook::{self, Flow, Orderbook, OrderbookError},
};
use petgraph::graph::NodeIndex;
use std::iter::FusedIterator;

/// An iterator over all transitive orders over a token pair for an orderbook,
/// ordered from lowest limit price to highest.
pub struct TransitiveOrders {
    orderbook: Orderbook,
    /// The token pair converted to graph node indices. This valud is `None` if
    /// the token pair is invalid.
    pair: Option<(NodeIndex, NodeIndex)>,
    /// The first order is always computed when the iterator is created to check
    /// whether the orderbook is reduced in the subgraph containing the `buy`
    /// token.
    first_order: Option<(Path<NodeIndex>, Flow)>,
    /// The number of hops that can be considered during path finding (None being infinite)
    hops: Option<usize>,
    errored: bool,
}

impl TransitiveOrders {
    /// Creates a new transitive orderbook iterator.
    pub fn new(orderbook: Orderbook, pair_range: TokenPairRange) -> Result<Self, OrderbookError> {
        let (buy, sell) = if orderbook.is_token_pair_valid(pair_range.pair) {
            (
                orderbook::node_index(pair_range.pair.buy),
                orderbook::node_index(pair_range.pair.sell),
            )
        } else {
            return Ok(Self {
                orderbook,
                pair: None,
                first_order: None,
                hops: None,
                errored: false,
            });
        };

        // NOTE: We need to check that the orderbook is not overlapping in the
        // subgraph containing the token pair we care about, so we find the
        // first transitive order and reuse the result in the first call to
        // `next`.
        let first_order = orderbook.find_path_and_flow(buy, sell, pair_range.hops)?;

        Ok(Self {
            orderbook,
            pair: Some((buy, sell)),
            first_order,
            hops: pair_range.hops,
            errored: false,
        })
    }
}

impl Iterator for TransitiveOrders {
    type Item = Result<Flow, OrderbookError>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.errored {
            return None;
        }
        let (buy, sell) = self.pair?;
        let (path, flow) = match self.first_order.take() {
            Some(order) => order,
            None => match self.orderbook.find_path_and_flow(buy, sell, self.hops) {
                Ok(order) => order?,
                Err(err) => {
                    self.errored = true;
                    return Some(Err(err));
                }
            },
        };

        if let Err(err) = self.orderbook.fill_path_with_flow(&path, &flow) {
            self.errored = true;
            return Some(Err(err));
        }
        Some(Ok(flow))
    }
}

impl FusedIterator for TransitiveOrders {}

#[cfg(test)]
mod tests {
    use crate::encoding::TokenPair;
    use crate::test::prelude::*;
    use crate::FEE_FACTOR;

    #[test]
    fn iterates_transitive_orders() {
        // 0 --1.0--> 1 --1.0--> 2
        //  \                    ^
        //   \-------2.0--------/
        //    \------3.0-------/
        let orderbook = orderbook! {
            users {
                @1 {
                    token 1 => 100_000_000,
                }
                @2 {
                    token 2 => 100_000_000,
                }
            }
            orders {
                owner @1 buying 0 [1_000_000] selling 1 [1_000_000],
                owner @2 buying 1 [1_000_000] selling 2 [1_000_000],
                owner @2 buying 0 [2_000_000] selling 2 [1_000_000],
                owner @2 buying 0 [3_000_000] selling 2 [1_000_000],
            }
        };
        let pair = TokenPair { buy: 0, sell: 2 }.into_unbounded_range();

        let orders = orderbook
            .clone()
            .transitive_orders(pair)
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();

        assert_eq!(orders.len(), 3);

        assert_approx_eq!(orders[0].exchange_rate.value(), 1.0 * FEE_FACTOR.powi(2));
        assert_approx_eq!(orders[0].capacity, 1_000_000.0 * FEE_FACTOR);
        assert_approx_eq!(orders[0].min_trade, 1_000_000.0 / FEE_FACTOR);

        assert_approx_eq!(orders[1].exchange_rate.value(), 2.0 * FEE_FACTOR);
        assert_approx_eq!(orders[1].capacity, 2_000_000.0 * FEE_FACTOR);
        assert_approx_eq!(orders[1].min_trade, 1_000_000.0);

        assert_approx_eq!(orders[2].exchange_rate.value(), 3.0 * FEE_FACTOR);
        assert_approx_eq!(orders[2].capacity, 3_000_000.0 * FEE_FACTOR);
        assert_approx_eq!(orders[2].min_trade, 1_000_000.0);

        // NOTE: Orderbook is reduced, so it should produce the same results.
        assert_eq!(
            orders,
            orderbook
                .reduce_overlapping_orders()
                .unwrap()
                .transitive_orders(pair)
                .collect::<Result<Vec<_>, _>>()
                .unwrap(),
        );
    }

    #[test]
    fn returns_error_for_overlapping_orderbook() {
        //  /---0.5---v
        // 0          1
        //  ^---0.5---/
        let orderbook = orderbook! {
            users {
                @0 {
                    token 0 => 10_000_000,
                }
                @1 {
                    token 1 => 10_000_000,
                }
            }
            orders {
                owner @0 buying 1 [5_000_000] selling 0 [10_000_000],
                owner @1 buying 0 [5_000_000] selling 1 [10_000_000],
            }
        };
        let pair = TokenPair { buy: 1, sell: 0 }.into_unbounded_range();

        assert!(orderbook.is_overlapping());
        assert!(orderbook.transitive_orders(pair).is_err());
    }
}
