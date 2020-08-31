//! Module containing transitive order iterator for an orderbook.

use crate::{
    encoding::TokenPair,
    graph::path::Path,
    orderbook::{self, Flow, Orderbook, OverlapError, ReducedOrderbook},
};
use petgraph::graph::NodeIndex;
use std::{iter::FusedIterator, mem};

/// An iterator over all transitive orders over a token pair for an orderbook,
/// ordered from lowest limit price to highest.
pub struct TransitiveOrders {
    orderbook: Orderbook,
    pair: Option<(NodeIndex, NodeIndex)>,
    state: State,
}

/// Internal iterator state.
enum State {
    /// The next transitive order has already been pre-computed and is ready to
    /// be returned.
    NextOrder(Option<(Path<NodeIndex>, Flow)>),
    /// The previous order needs to be filled before continuing.
    FillPreviousOrder(Path<NodeIndex>, Flow),
    /// The next transitive order needs to be found, and no previous orders
    /// need to be filled.
    Search,
}

impl TransitiveOrders {
    /// Creates an empty transitive orderbook iterator.
    fn empty(orderbook: Orderbook) -> Self {
        Self {
            orderbook,
            pair: None,
            state: State::Search,
        }
    }

    /// Creates a new transitive orderbook iterator.
    pub fn new(orderbook: Orderbook, pair: TokenPair) -> Result<Self, OverlapError> {
        let (buy, sell) = if orderbook.is_token_pair_valid(pair) {
            (
                orderbook::node_index(pair.buy),
                orderbook::node_index(pair.sell),
            )
        } else {
            return Ok(Self::empty(orderbook));
        };

        // NOTE: We need to check that the orderbook is not overlapping in the
        // subgraph containing the token pair we care about, so we find the
        // first transitive order and reuse the result in the first call to
        // `next`.
        let next_order = orderbook.find_path_and_flow(buy, sell)?;

        Ok(Self {
            orderbook,
            pair: Some((buy, sell)),
            state: State::NextOrder(next_order),
        })
    }

    /// Creates a new transitive orderbook iterator from a reduced orderbook.
    pub fn from_reduced(orderbook: ReducedOrderbook, pair: TokenPair) -> Self {
        let orderbook = orderbook.into_inner();
        let pair = if orderbook.is_token_pair_valid(pair) {
            (
                orderbook::node_index(pair.buy),
                orderbook::node_index(pair.sell),
            )
        } else {
            return Self::empty(orderbook);
        };

        Self {
            orderbook,
            pair: Some(pair),
            state: State::Search,
        }
    }
}

impl Iterator for TransitiveOrders {
    type Item = Flow;

    fn next(&mut self) -> Option<Self::Item> {
        let (buy, sell) = self.pair?;
        let (path, flow) = match mem::replace(&mut self.state, State::Search) {
            State::NextOrder(order) => order?,
            State::FillPreviousOrder(path, flow) => {
                self.orderbook
                    .fill_path_with_flow(&path, &flow)
                    .unwrap_or_else(|| {
                        panic!(
                            "failed to fill with capacity along detected path {}",
                            orderbook::format_path(&path),
                        )
                    });

                self.orderbook
                    .find_path_and_flow(buy, sell)
                    .expect("negative cycle after computing shortest path")?
            }
            State::Search => self
                .orderbook
                .find_path_and_flow(buy, sell)
                .expect("negative cycle after computing shortest path")?,
        };

        self.state = State::FillPreviousOrder(path, flow);
        Some(flow)
    }
}

impl FusedIterator for TransitiveOrders {}

#[cfg(test)]
mod tests {
    use super::*;
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
        let pair = TokenPair { buy: 0, sell: 2 };

        let orders = orderbook
            .clone()
            .transitive_orders(pair)
            .unwrap()
            .collect::<Vec<_>>();

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
                .transitive_orders(pair)
                .collect::<Vec<_>>(),
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
        let pair = TokenPair { buy: 1, sell: 0 };

        assert!(orderbook.is_overlapping());
        assert!(orderbook.transitive_orders(pair).is_err());
    }
}
