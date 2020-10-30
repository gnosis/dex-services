//! Implementation of a graph representation of an orderbook where tokens are
//! vertices and orders are edges (with users and balances as auxiliary data
//! to these edges).
//!
//! Storage is optimized for graph-related operations such as listing the edges
//! (i.e. orders) connecting a token pair.

mod flow;
mod iter;
mod map;
mod order;
mod reduced;
mod scalar;
mod user;
mod weight;

pub use self::flow::{Flow, Ring};
pub use self::iter::TransitiveOrders;
use self::order::{Amount, Order, OrderCollector, OrderMap};
pub use self::reduced::ReducedOrderbook;
pub use self::scalar::{ExchangeRate, LimitPrice};
use self::user::{User, UserMap};
pub use self::weight::Weight;
use crate::api::Market;
use crate::encoding::{Element, TokenId, TokenPair};
use crate::graph::path::{NegativeCycle, Path};
use crate::graph::shortest_paths::shortest_path;
use crate::graph::subgraph::{ControlFlow, Subgraphs};
use crate::num;
use petgraph::graph::{DiGraph, EdgeIndex, NodeIndex};
use petgraph::visit::NodeIndexable;
use primitive_types::U256;
use std::cmp;
use std::f64;
use thiserror::Error;

type OrderbookGraph = DiGraph<TokenId, Weight>;

/// A graph representation of a complete orderbook.
#[derive(Clone, Debug)]
pub struct Orderbook {
    /// A map of sell tokens to a mapping of buy tokens to orders such that
    /// `orders[sell][buy]` is a vector of orders selling token `sell` and
    /// buying token `buy`.
    orders: OrderMap,
    /// Auxiliary user data containing user balances and order counts. Balances
    /// are important as they affect the capacity of an edge between two tokens.
    users: UserMap,
    /// A projection of the orderbook onto a graph with nodes as tokens and
    /// edges as the lowest order exchange rate between token pairs.
    projection: OrderbookGraph,
}

impl Orderbook {
    /// Creates an orderbook from an iterator over decoded auction elements.
    pub fn from_elements(elements: impl IntoIterator<Item = Element>) -> Self {
        let mut max_token = 0;
        let mut orders = OrderCollector::default();
        let mut users = UserMap::default();

        for (order, element) in elements
            .into_iter()
            .filter(|element| !is_dust_order(element))
            .filter(|element| element.pair.buy != element.pair.sell)
            .filter_map(|element| Order::new(&element).map(move |order| (order, element)))
        {
            let TokenPair { buy, sell } = element.pair;
            max_token = cmp::max(max_token, cmp::max(buy, sell));
            users.entry(element.user).or_default().set_balance(&element);
            orders.insert_order(order);
        }

        let orders = orders.collect();
        let mut projection = DiGraph::new();
        for token_id in 0..=max_token {
            let token_node = projection.add_node(token_id);

            // NOTE: Tokens are added in order such that token_id == token_node
            // index, assert that the node index is indeed what we expect it to
            // be.
            debug_assert_eq!(token_node, node_index(token_id));
        }
        projection.extend_with_edges(orders.all_pairs().map({
            |(pair, orders)| {
                let cheapest_order = orders
                    .last()
                    .expect("unexpected token pair in orders map without any orders");
                (
                    node_index(pair.buy),
                    node_index(pair.sell),
                    cheapest_order.exchange_rate.weight(),
                )
            }
        }));

        Orderbook {
            orders,
            users,
            projection,
        }
    }

    /// Returns the number of orders in the orderbook.
    pub fn num_orders(&self) -> usize {
        self.orders.all_pairs().map(|(_, o)| o.len()).sum()
    }

    /// Detects whether the orderbook is overlapping, that is if the orderbook's
    /// projection graph contains any negative cycles.
    ///
    /// Conceptually, a negative cycle is a trading path starting and ending at
    /// a token (going through an arbitrary number of other distinct tokens)
    /// where the total weight is less than `0`, i.e. the transitive exchange
    /// rate is less than `1`. This means that there is a price overlap along
    /// this ring trade.
    pub fn is_overlapping(&self) -> bool {
        // NOTE: We detect negative cycles from each disconnected subgraph.
        Subgraphs::new(self.projection.node_indices().skip(1))
            .for_each_until(|token| match shortest_path(&self.projection, token, None) {
                Ok(shortest_path_graph) => {
                    ControlFlow::Continue(shortest_path_graph.connected_nodes())
                }
                Err(_) => ControlFlow::Break(true),
            })
            .unwrap_or(false)
    }

    /// Reduces the orderbook by matching all overlapping ring trades.
    pub fn reduce_overlapping_orders(mut self) -> ReducedOrderbook {
        Subgraphs::new(self.projection.node_indices()).for_each(|token| loop {
            let cycle = match shortest_path(&self.projection, token, None) {
                Ok(shortest_path_graph) => break shortest_path_graph.connected_nodes(),
                Err(cycle) => cycle,
            };
            self.fill_path(&cycle).unwrap_or_else(|| {
                panic!(
                    "failed to fill path along detected negative cycle {}",
                    format_path(&cycle),
                )
            });
        });

        debug_assert!(!self.is_overlapping());
        ReducedOrderbook(self)
    }

    /// Fills a ring trade over the specified market, and returns the flow
    /// corresponding to both ask and bid segments of the ring. Returns `None`
    /// if there are no overlapping ring trades over the specified market.
    ///
    /// Note that if this method returns `None`, then the orderbook is
    /// **partially** reduced. Specifically, the subgraph containing the market
    /// `quote` token is fully reduced, however, other not connected subgraphs,
    /// specifically the market `base`'s subgraph in the case where the `quote`
    /// and `base` token are not part of the same subgraph, may still contain
    /// negative cycles.
    pub fn fill_market_ring_trade(&mut self, market: Market) -> Option<Ring> {
        if !self.is_token_pair_valid(market.bid_pair()) {
            return None;
        }

        let (base, quote) = (node_index(market.base), node_index(market.quote));

        loop {
            let cycle = match shortest_path(&self.projection, quote, None) {
                Ok(_) => break,
                Err(cycle) => cycle,
            };
            let paths_base_quote = cycle
                .with_starting_node(quote)
                .and_then(|cycle| cycle.split_at(base));
            match paths_base_quote {
                Ok((ask, bid)) => {
                    let ask = self
                        .fill_path(&ask)
                        .expect("ask transitive path not found after being detected");
                    let bid = self
                        .fill_path(&bid)
                        .expect("bid transitive path not found after being detected");

                    return Some(Ring { ask, bid });
                }
                Err(cycle) => {
                    // NOTE: Skip negative cycles that are not along the
                    // specified market.
                    self.fill_path(&cycle).unwrap_or_else(|| {
                        panic!(
                            "failed to fill path along detected negative cycle {}",
                            format_path(&cycle),
                        )
                    });
                }
            };
        }

        None
    }

    /// Returns an iterator over all transitive orders from lowest to highest
    /// limit price for the orderbook.
    ///
    /// Returns an error if the orderbook is not reduced in the subgraph
    /// containing the token pair's buy token, i.e. one or more negative cycles
    /// were found when searching for the shortest path starting from the buy
    /// token and ending at the sell token.
    pub fn transitive_orders(self, pair: TokenPair) -> Result<TransitiveOrders, OverlapError> {
        TransitiveOrders::new(self, pair)
    }

    /// Finds and returns the optimal transitive order for the specified token
    /// pair without filling it. Returns `None` if no such transitive order
    /// exists.
    ///
    /// This method returns an error if the orderbook graph is not reduced.
    pub fn find_optimal_transitive_order(
        &self,
        pair: TokenPair,
    ) -> Result<Option<Flow>, OverlapError> {
        if !self.is_token_pair_valid(pair) {
            return Ok(None);
        }

        let (start, end) = (node_index(pair.buy), node_index(pair.sell));
        let flow = match self.find_path_and_flow(start, end)? {
            Some((_, flow)) => flow,
            None => return Ok(None),
        };

        Ok(Some(flow))
    }

    /// Updates all projection graph edges that enter a token.
    fn update_projection_graph_node(&mut self, sell: TokenId) {
        let pairs = self
            .orders
            .pairs_and_orders_for_sell_token(sell)
            .map(|(pair, _)| pair)
            .collect::<Vec<_>>();
        for pair in pairs {
            self.update_projection_graph_edge(pair);
        }
    }

    /// Updates the projection graph edge between a token pair.
    ///
    /// This is done by removing all filled orders, i.e. orders whose remaining
    /// amount or whose users remaining balance is zero, and then either
    /// updating the projection graph edge with the weight of the new cheapest
    /// order or removing the edge entirely if no orders remain for the given
    /// token pair.
    fn update_projection_graph_edge(&mut self, pair: TokenPair) {
        while let Some(true) = self.orders.best_order_for_pair(pair).map(|order| {
            num::is_dust_amount(num::u256_to_u128_saturating(
                order.get_effective_amount(&self.users),
            ))
        }) {
            self.orders.remove_pair_order(pair);
        }

        let edge = self.get_pair_edge(pair).unwrap_or_else(|| {
            panic!(
                "missing edge between token pair {}->{} with orders",
                pair.buy, pair.sell
            )
        });
        if let Some(order) = self.orders.best_order_for_pair(pair) {
            self.projection[edge] = order.exchange_rate.weight();
        } else {
            self.projection.remove_edge(edge);
        }
    }

    /// Retrieve the edge index in the projection graph for a token pair,
    /// returning `None` when the edge does not exist.
    fn get_pair_edge(&self, pair: TokenPair) -> Option<EdgeIndex> {
        let (buy, sell) = (node_index(pair.buy), node_index(pair.sell));
        self.projection.find_edge(buy, sell)
    }

    /// Finds and fills a trading path through the orderbook between the
    /// specified tokens and computes the flow for the path.
    fn find_path_and_flow(
        &self,
        start: NodeIndex,
        end: NodeIndex,
    ) -> Result<Option<(Path<NodeIndex>, Flow)>, OverlapError> {
        let shortest_path_graph = shortest_path(&self.projection, start, None)?;
        let path = match shortest_path_graph.path_to(end) {
            Some(path) => path,
            None => return Ok(None),
        };

        let flow = self.find_path_flow(&path).unwrap_or_else(|| {
            panic!(
                "failed to fill detected shortest path {}",
                format_path(&path),
            )
        });

        Ok(Some((path, flow)))
    }

    /// Fills a trading path through the orderbook to maximum capacity, reducing
    /// the remaining order amounts and user balances along the way, returning
    /// the flow along the trading path or `None` if the path was invalid.
    ///
    /// Note that currently, user buy token balances are not incremented as a
    /// result of filling orders along a path.
    fn fill_path(&mut self, path: &[NodeIndex]) -> Option<Flow> {
        let flow = self.find_path_flow(path)?;
        self.fill_path_with_flow(path, &flow).unwrap_or_else(|| {
            panic!(
                "failed to fill with capacity along detected path {}",
                format_path(path),
            )
        });

        Some(flow)
    }

    /// Finds a transitive trade along a path and returns the corresponding flow
    /// for that path or `None` if the path doesn't exist.
    fn find_path_flow(&self, path: &[NodeIndex]) -> Option<Flow> {
        // NOTE: Capacity is expressed in the starting token, which is the buy
        // token for the transitive order along the specified path.
        let mut capacity = f64::INFINITY;
        let mut transitive_xrate = ExchangeRate::IDENTITY;
        let mut max_xrate = ExchangeRate::IDENTITY;
        for pair in pairs_on_path(path) {
            let order = self.orders.best_order_for_pair(pair)?;
            transitive_xrate *= order.exchange_rate;
            max_xrate = cmp::max(max_xrate, transitive_xrate);

            let sell_amount = num::u256_to_f64(order.get_effective_amount(&self.users));
            capacity = num::min(capacity, sell_amount * transitive_xrate.value());
        }

        Some(Flow {
            exchange_rate: transitive_xrate,
            capacity,
            min_trade: capacity / max_xrate.value(),
        })
    }

    /// Pushes flow through a path of orders reducing order amounts and user
    /// balances as well as updating the projection graph by updating the
    /// weights to reflect the new graph. Returns `None` if the path does not
    /// exist.
    ///
    /// Note that currently, user buy token balances are not incremented as a
    /// result of filling orders along a path.
    fn fill_path_with_flow(&mut self, path: &[NodeIndex], flow: &Flow) -> Option<()> {
        let mut transitive_xrate = ExchangeRate::IDENTITY;
        for pair in pairs_on_path(path) {
            let (order, user) = self.best_order_with_user_for_pair_mut(pair)?;

            transitive_xrate *= order.exchange_rate;

            // NOTE: `capacity` is expressed in the buy token, so we need to
            // divide by the exchange rate to get the sell amount being filled.
            let fill_amount = num::f64_to_u256(flow.capacity / transitive_xrate.value());
            let new_balance = user.deduct_from_balance(pair.sell, fill_amount);

            if num::is_dust_amount(num::u256_to_u128_saturating(new_balance)) {
                user.clear_balance(pair.sell);
                self.update_projection_graph_node(pair.sell);
            } else if let Amount::Remaining(amount) = &mut order.amount {
                // TODO: add a debug assert to see that we are not over filling orders.
                // Will do this when we use BigRational.
                *amount = amount.saturating_sub(num::u256_to_u128_saturating(fill_amount));
                if num::is_dust_amount(*amount) {
                    self.update_projection_graph_edge(pair);
                }
            }
        }

        Some(())
    }

    /// Gets a mutable reference to the cheapest order for a given token pair
    /// along with the user that placed the order. Returns `None` if there are
    /// no orders for that token pair.
    fn best_order_with_user_for_pair_mut(
        &mut self,
        pair: TokenPair,
    ) -> Option<(&'_ mut Order, &'_ mut User)> {
        let Self { orders, users, .. } = self;

        let order = orders.best_order_for_pair_mut(pair)?;
        let user = users
            .get_mut(&order.user)
            .unwrap_or_else(|| panic!("missing user {:?} for existing order", order.user));

        Some((order, user))
    }

    /// Returns whether the specified token pair is valid.
    ///
    /// A token pair is considered valid if both the buy and sell token
    /// exist in the current orderbook and are unequal.
    fn is_token_pair_valid(&self, pair: TokenPair) -> bool {
        let node_bound = self.projection.node_bound();
        pair.buy != pair.sell
            && (pair.buy as usize) < node_bound
            && (pair.sell as usize) < node_bound
    }
}

/// Create a node index from a token ID.
fn node_index(token: TokenId) -> NodeIndex {
    NodeIndex::new(token as _)
}

/// Create a token ID from a node index.
fn token_id(node: NodeIndex) -> TokenId {
    node.index() as _
}

/// Returns an iterator with all pairs on a path.
fn pairs_on_path(path: &[NodeIndex]) -> impl Iterator<Item = TokenPair> + '_ {
    path.windows(2).map(|segment| TokenPair {
        buy: token_id(segment[0]),
        sell: token_id(segment[1]),
    })
}

/// Formats a token path into a string.
fn format_path(path: &[NodeIndex]) -> String {
    path.iter()
        .map(|segment| segment.index().to_string())
        .collect::<Vec<_>>()
        .join("->")
}

/// Returns true if an auction element is a "dust" order, i.e. their remaining
/// amount or balance is less than the minimum amount that the exchange allows
/// for trades
fn is_dust_order(element: &Element) -> bool {
    num::is_dust_amount(element.remaining_sell_amount as _)
        || (num::is_dust_amount(element.balance.low_u128())
            && element.balance < U256::from(u128::MAX))
}

/// An error indicating an invalid operation was performed on an overlapping
/// orderbook.
#[derive(Debug, Error)]
#[error("invalid operation on an overlapping orderbook")]
pub struct OverlapError(pub NegativeCycle<NodeIndex>);

impl From<NegativeCycle<NodeIndex>> for OverlapError {
    fn from(cycle: NegativeCycle<NodeIndex>) -> Self {
        OverlapError(cycle)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test::prelude::*;
    use crate::FEE_FACTOR;
    use petgraph::algo::FloatMeasure;

    impl Orderbook {
        /// Retrieve the weight of an edge in the projection graph. This is used for
        /// testing that the projection graph is in sync with the order map.
        fn get_projected_pair_weight(&self, pair: TokenPair) -> Weight {
            let edge = match self.get_pair_edge(pair) {
                Some(edge) => edge,
                None => return Weight::infinite(),
            };
            self.projection[edge]
        }
    }

    #[test]
    fn reduces_overlapping_orders() {
        //             /---0.5---v
        // 0 <--1.0-- 1          2 --1.0--> 3 --1.0--> 4 --1.0--> 5
        //            ^---1.0---/           ^                    /
        //                                   \--------0.5-------/
        //             /---0.1---v
        //            6          7
        //            ^---1.0---/
        let orderbook = orderbook! {
            users {
                @0 {
                    token 0 => 10_000_000,
                }
                @1 {
                    token 1 => 20_000_000,
                    token 3 => 10_000_000,
                    token 4 => 10_000_000,
                    token 5 => 20_000_000,
                }
                @2 {
                    token 2 => 1_000_000_000,
                    token 3 => 1_000_000_000,
                }

                @3 {
                    token 6 => 1_000_000,
                }
                @4 {
                    token 7 => 1_000_000,
                }
            }
            orders {
                owner @0 buying 1 [10_000_000] selling 0 [10_000_000],

                owner @1 buying 2 [10_000_000] selling 1 [10_000_000] (1_000_000),
                owner @1 buying 2 [10_000_000] selling 3 [10_000_000],
                owner @1 buying 3 [10_000_000] selling 4 [10_000_000],
                owner @1 buying 4 [10_000_000] selling 5 [10_000_000],

                owner @2 buying 1 [5_000_000] selling 2 [10_000_000],
                owner @2 buying 5 [5_000_000] selling 3 [10_000_000],

                owner @3 buying 7 [10_000_000] selling 6 [10_000_000],
                owner @4 buying 6 [1_000_000] selling 7 [10_000_000],
            }
        };

        let ReducedOrderbook(orderbook) = orderbook.reduce_overlapping_orders();
        // NOTE: We expect user 1's 2->1 order to be completely filled as well
        // as user 2's 5->3 order and user 4's 6->7 order.
        // User 3's 7->6 order may be filled or not depending on the order in
        // which the loop {6, 7} is cleared:
        // 6->7->6: 100_000 T6 -> 1_000_000 T7 -> 1_000_000 T6, both orders cleared
        // 7->6->7: 100_000 T7 -> 100_000 T6 -> 1_000_000 T7, only 6->7 cleared
        assert!(orderbook.num_orders() == 5 || orderbook.num_orders() == 6);
        assert!(!orderbook.is_overlapping());
    }

    #[test]
    fn path_finding_operations_fail_on_overlapping_orders() {
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
        assert!(orderbook.find_optimal_transitive_order(pair).is_err());
    }

    #[test]
    fn removes_dust_orders() {
        let orderbook = orderbook! {
            users {
                @1 {
                    token 0 => 1_000_000_000,
                }
                @2 {
                    token 1 => 4_999_000,
                }
                @3 {
                    token 0 => 9_000,
                    token 1 => 1_000_000_000,
                }
            }
            orders {
                owner @1 buying 1 [1_000_000_000] selling 0 [1_000_000_000],
                owner @2 buying 0 [1_000_000_000] selling 1 [2_000_000_000],
                owner @3 buying 1 [1_000_000_000] selling 0 [3_000_000_000],
                owner @3 buying 0 [1_000_000_000] selling 1 [3_000_000_000] (0),
            }
        };

        let pair = TokenPair { buy: 0, sell: 1 };

        let order = orderbook.orders.best_order_for_pair(pair).unwrap();
        assert_eq!(orderbook.num_orders(), 2);
        assert_eq!(order.user, user_id(2));
        assert_eq!(
            order.exchange_rate.weight(),
            orderbook.get_projected_pair_weight(pair)
        );

        let ReducedOrderbook(orderbook) = orderbook.reduce_overlapping_orders();
        let order = orderbook.orders.best_order_for_pair(pair);
        assert_eq!(orderbook.num_orders(), 1);
        assert!(order.is_none());
        assert_eq!(
            orderbook.get_projected_pair_weight(pair),
            Weight::infinite()
        );
    }

    #[test]
    fn ignores_orders_with_invalid_prices() {
        let orderbook = orderbook! {
            users {
                @1 {
                    token 0 => 1_000_000_000,
                }
            }
            orders {
                owner @1 buying 1 [1_000_000_000] selling 0 [0],
                owner @1 buying 1 [0] selling 0 [1_000_000_000],
            }
        };

        assert_eq!(orderbook.num_orders(), 0);
    }

    #[test]
    fn ignores_orders_with_invalid_market() {
        let orderbook = orderbook! {
            users {
                @1 {
                    token 0 => 1_000_000_000,
                }
            }
            orders {
                owner @1 buying 0 [1_000_000_000] selling 0 [1_000_000_000],
            }
        };

        assert_eq!(orderbook.num_orders(), 0);
    }

    #[test]
    fn fills_path_and_updates_projection() {
        //             /---1.0---v
        // 0 --1.0--> 1          2 --1.0--> 3 --0.5--> 4
        //             \---2.0---^                    /
        //                        \--------1.0-------/
        let mut orderbook = orderbook! {
            users {
                @1 {
                    token 1 => 1_000_000,
                }
                @2 {
                    token 2 => 500_000,
                }
                @3 {
                    token 3 => 1_000_000,
                }
                @4 {
                    token 4 => 10_000_000,
                }
                @5 {
                    token 2 => 1_000_000,
                }
            }
            orders {
                owner @1 buying 0 [1_000_000] selling 1 [1_000_000],
                owner @2 buying 1 [1_000_000] selling 2 [1_000_000],
                owner @2 buying 4 [1_000_000] selling 2 [1_000_000],
                owner @3 buying 2 [1_000_000] selling 3 [1_000_000] (500_000),
                owner @4 buying 3 [1_000_000] selling 4 [2_000_000],
                owner @5 buying 1 [2_000_000] selling 2 [1_000_000],
            }
        };

        let path = [0, 1, 2, 3, 4]
            .iter()
            .copied()
            .map(node_index)
            .collect::<Vec<_>>();

        let flow = orderbook.find_path_flow(&path).unwrap();
        assert_approx_eq!(
            flow.capacity,
            // NOTE: We can send a little more than the balance limit of user 2
            // because some of it gets eaten up by the fees along the way.
            500_000.0 * FEE_FACTOR.powi(2)
        );
        assert_approx_eq!(flow.exchange_rate.value(), 0.5 * FEE_FACTOR.powi(4));

        let filled = orderbook.fill_path(&path).unwrap();
        assert_eq!(filled, flow);

        // NOTE: The expected fill amounts are:
        //  Order | buy amt | sell amt | xrate
        // -------+---------+----------+-------
        // 0 -> 1 | 501_000 |  500_500 |   1.0
        // 1 -> 2 | 500_500 |  500_000 |   1.0
        // 2 -> 3 | 500_000 |  499_500 |   1.0
        // 3 -> 4 | 499_500 |  998_000 |   0.5

        assert_eq!(
            orderbook.get_projected_pair_weight(TokenPair { buy: 1, sell: 2 }),
            // NOTE: user 2's order has no more balance so expect the new weight
            // between tokens 1 and 2 to be user 5's order with the worse price
            // of 2:1 (meaning it needs twice as much token 1 to get the same
            // amount of token 2 when pushing flow through that edge).
            Weight::new(2.0 * FEE_FACTOR),
        );
        // NOTE: User 2's other order selling token 2 for 4 also has no more
        // balance so check that it was also removed from the order map and from
        // the projection graph.
        assert!(orderbook
            .orders
            .best_order_for_pair(TokenPair { buy: 4, sell: 2 })
            .is_none());
        assert_eq!(
            orderbook.get_projected_pair_weight(TokenPair { buy: 4, sell: 2 }),
            Weight::infinite(),
        );
        // NOTE: User 3's order selling token 3 for 2 has become a dust order
        // (since it has a remaining amount of about 500), check that it was
        // removed.
        assert!(orderbook
            .orders
            .best_order_for_pair(TokenPair { buy: 2, sell: 3 })
            .is_none());
        assert_eq!(
            orderbook.get_projected_pair_weight(TokenPair { buy: 2, sell: 3 }),
            Weight::infinite(),
        );

        assert_eq!(orderbook.num_orders(), 3);

        let transitive_xrate_0_1 = FEE_FACTOR;
        assert_eq!(
            orderbook
                .orders
                .best_order_for_pair(TokenPair { buy: 0, sell: 1 })
                .unwrap()
                .amount,
            Amount::Remaining(1_000_000 - (flow.capacity / transitive_xrate_0_1) as u128)
        );
        assert_eq!(
            orderbook.users[&user_id(1)].balance_of(1),
            U256::from(1_000_000 - (flow.capacity / transitive_xrate_0_1) as u128)
        );

        assert_eq!(
            orderbook
                .orders
                .best_order_for_pair(TokenPair { buy: 1, sell: 2 })
                .unwrap()
                .amount,
            Amount::Remaining(1_000_000),
        );
        assert_eq!(
            orderbook.users[&user_id(5)].balance_of(2),
            U256::from(1_000_000)
        );

        assert_eq!(
            orderbook
                .orders
                .best_order_for_pair(TokenPair { buy: 3, sell: 4 })
                .unwrap()
                .amount,
            Amount::Remaining(2_000_000 - (flow.capacity / flow.exchange_rate.value()) as u128)
        );
        assert_eq!(
            orderbook.users[&user_id(4)].balance_of(4),
            U256::from(10_000_000 - (flow.capacity / flow.exchange_rate.value()) as u128)
        );
    }

    #[test]
    fn search_panics_on_undetected_negative_cycle_due_to_rounding_errors() {
        //              /---250---v
        // 0 --1.11--> 1          2
        //             ^--0.004--/
        let orderbook = orderbook! {
            users {
                @1 {
                    token 1 => 10_000_000_000,
                }
                @2 {
                    token 2 => 10_000_000_000,
                }
            }
            orders {
                owner @1 buying 0 [1_000_000_000] selling 1 [1_000_000_000],
                owner @1 buying 2 [1_000_000] selling 1 [250_000_000],
                owner @2 buying 1 [249_500_250] selling 2 [1_000_000],
            }
        };

        assert!(orderbook
            .find_optimal_transitive_order(TokenPair { buy: 0, sell: 1 })
            .is_ok())
    }
}
