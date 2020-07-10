//! Implementation of a graph representation of an orderbook where tokens are
//! vertices and orders are edges (with users and balances as auxiliary data
//! to these edges).
//!
//! Storage is optimized for graph-related operations such as listing the edges
//! (i.e. orders) connecting a token pair.

mod flow;
mod order;
mod user;

use self::flow::Flow;
use self::order::{Order, OrderCollector, OrderMap};
use self::user::{User, UserMap};
use crate::encoding::{Element, TokenId, TokenPair};
use crate::graph::bellman_ford::{self, NegativeCycle};
use crate::graph::path;
use crate::graph::subgraph::{ControlFlow, Subgraphs};
use crate::num;
use crate::{Market, TransitiveOrder, TransitiveOrderbook, FEE_FACTOR};
use petgraph::graph::{DiGraph, EdgeIndex, NodeIndex};
use petgraph::visit::NodeIndexable;
use primitive_types::U256;
use std::cmp;
use std::f64;
use thiserror::Error;

/// The minimum amount where an order is considered a dust order and can be
/// ignored in the price graph calculation.
const MIN_AMOUNT: f64 = 10_000.0;

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
    /// A projection of the order book onto a graph of lowest priced orders
    /// between tokens.
    projection: DiGraph<TokenId, f64>,
}

impl Orderbook {
    /// Creates an orderbook from an iterator over decoded auction elements.
    pub fn from_elements(elements: impl IntoIterator<Item = Element>) -> Self {
        let mut max_token = 0;
        let mut orders = OrderCollector::default();
        let mut users = UserMap::default();

        for element in elements.into_iter().filter(should_include_auction_element) {
            let TokenPair { buy, sell } = element.pair;
            max_token = cmp::max(max_token, cmp::max(buy, sell));
            users.entry(element.user).or_default().set_balance(&element);
            orders.insert_order(Order::new(&element));
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
                    cheapest_order.weight(),
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
    /// where the total weight is less than `0`, i.e. the effective sell price
    /// is less than `1`. This means that there is a price overlap along this
    /// ring trade.
    pub fn is_overlapping(&self) -> bool {
        // NOTE: We detect negative cycles from each disconnected subgraph.
        Subgraphs::new(self.projection.node_indices().skip(1))
            .for_each_until(
                |token| match bellman_ford::search(&self.projection, token) {
                    Ok((_, predecessor)) => ControlFlow::Continue(predecessor),
                    Err(NegativeCycle(..)) => ControlFlow::Break(true),
                },
            )
            .unwrap_or(false)
    }

    /// Reduces the orderbook by matching all overlapping ring trades.
    pub fn reduce_overlapping_orders(&mut self) {
        Subgraphs::new(self.projection.node_indices()).for_each(|token| loop {
            match bellman_ford::search(&self.projection, token) {
                Ok((_, predecessors)) => break predecessors,
                Err(NegativeCycle(predecessors, node)) => {
                    let path = path::find_cycle(&predecessors, node, None)
                        .expect("negative cycle not found after being detected");

                    self.fill_path(&path).unwrap_or_else(|| {
                        panic!(
                            "failed to fill path along detected negative cycle {}",
                            format_path(&path),
                        )
                    });
                }
            }
        });

        debug_assert!(!self.is_overlapping());
    }

    /// Find negative cycles in the specified market and splits each one into a
    /// `base -> quote` (ask) transitive order and a `quote -> base` (bid)
    /// transitive order, reducing them both. Returns the computed overlapping
    /// transitive orderbook for that market.
    ///
    /// Note that there may exist additional transitive orders that may overlap
    /// with orders in the resulting transitive orderbook. However, all negative
    /// cycles will be removed from the orderbook graph.
    pub fn reduce_overlapping_transitive_orderbook(
        &mut self,
        market: Market,
    ) -> TransitiveOrderbook {
        if !self.is_token_pair_valid(market.bid_pair()) {
            return Default::default();
        }

        let (base, quote) = (node_index(market.base), node_index(market.quote));

        let mut overlap = TransitiveOrderbook::default();
        while let Err(NegativeCycle(predecessors, node)) =
            bellman_ford::search(&self.projection, quote)
        {
            let path = path::find_cycle(&predecessors, node, Some(quote))
                .expect("negative cycle not found after being detected");

            if path.first() == Some(&quote) {
                if let Some(base_index) = path.iter().position(|node| *node == base) {
                    let (ask, bid) = (&path[0..base_index + 1], &path[base_index..]);

                    let (capacity, transitive_price) = self
                        .fill_path(ask)
                        .expect("ask transitive path not found after being detected");
                    overlap.asks.push(transitive_order_from_capacity_and_price(
                        capacity,
                        transitive_price,
                    ));

                    let (capacity, transitive_price) = self
                        .fill_path(bid)
                        .expect("bid transitive path not found after being detected");
                    overlap.bids.push(transitive_order_from_capacity_and_price(
                        capacity,
                        transitive_price,
                    ));

                    continue;
                }
            }

            self.fill_path(&path).unwrap_or_else(|| {
                panic!(
                    "failed to fill path along detected negative cycle {}",
                    format_path(&path),
                )
            });
        }

        overlap
    }

    /// Fills transitive orders along a token pair, optionally specifying a
    /// maximum price spread for the orders.
    ///
    /// Returns a vector containing all the transitive orders that were filled.
    ///
    /// Note that the spread is a decimal fraction that defines the maximum
    /// transitive order price with the equation:
    /// `first_transitive_price + first_transitive_price * spread`. This means
    /// that given a spread of 0.5 (or 50%), and if the cheapest transitive
    /// order has a price of 1.2, then the maximum price will be `1.8`.
    ///
    /// # Panics
    ///
    /// This method panics if the spread is zero or negative.
    pub fn fill_transitive_orders(
        &mut self,
        pair: TokenPair,
        spread: Option<f64>,
    ) -> Result<Vec<TransitiveOrder>, OverlapError> {
        if let Some(spread) = spread {
            assert!(spread > 0.0, "invalid spread");
        }

        let mut orders = Vec::new();
        let mut spread_limit_price = None;

        while let Some(flow) = self.fill_optimal_transitive_order_if(pair, |flow| {
            if let Some(spread) = spread {
                let spread_limit_price =
                    spread_limit_price.get_or_insert_with(|| flow.exchange_rate * (1.0 + spread));
                flow.exchange_rate <= *spread_limit_price
            } else {
                true
            }
        })? {
            orders.push(flow.as_transitive_order());
        }

        Ok(orders)
    }

    /// Fill a market order in the current orderbook graph returning the maximum
    /// price the order can have while overlapping with existing orders. Returns
    /// `None` if the order cannot be filled because the token pair is not
    /// connected, or the connection cannot support enough capacity to fully
    /// fill the specified volume.
    ///
    /// This is done by pushing the specified volume as a flow through the graph
    /// and finding the min-cost paths that have enough capacity to carry the
    /// flow from the sell token to the buy token. The min-cost is calculated
    /// using the successive shortest path algorithm with Bellman-Ford.
    ///
    /// Note that in general this method is not idempotent, successive calls
    /// with the same token pair and volume may yield different results as
    /// orders get filled with each match changing the orderbook.
    pub fn fill_market_order(
        &mut self,
        pair: TokenPair,
        volume: f64,
    ) -> Result<Option<f64>, OverlapError> {
        // NOTE: This method works by searching for the "best" counter
        // transitive orders, as such we need to search for transitive orders
        // in the inverse direction: from sell token to the buy token.
        let inverse_pair = TokenPair {
            buy: pair.sell,
            sell: pair.buy,
        };

        let mut last_exchange_rate = None;
        if volume <= 0.0 {
            // NOTE: For a 0 volume we simulate sending an tiny epsilon of value
            // through the network without actually filling any orders.
            self.fill_optimal_transitive_order_if(inverse_pair, |flow| {
                last_exchange_rate = Some(flow.exchange_rate);
                false
            })?;
        }

        let mut remaining_volume = volume;
        while remaining_volume > 0.0 {
            let flow = match self.fill_optimal_transitive_order(inverse_pair)? {
                Some(value) => value,
                None => return Ok(None),
            };
            last_exchange_rate = Some(flow.exchange_rate);
            remaining_volume -= flow.capacity;
        }

        // NOTE: The exchange rates are for transitive orders in the inverse
        // direction, so we need to invert the exchange rate and account for
        // the fees so that the estimated exchange rate actually overlaps with
        // the last counter transtive order's exchange rate.
        Ok(last_exchange_rate.map(|xrate| 1.0 / (xrate * FEE_FACTOR)))
    }

    /// Fill an order at a given exchange rate, returning a buy and sell amount
    /// such that an order with these amounts would be completely overlapping
    /// the current existing orders.
    ///
    /// This is calculated by repeatedly finding the optimal counter transitive
    /// orders whose limit exchange rates overlap with the specified limit
    /// exchange rate.
    ///
    /// Note that the limit exchange rate implicitly includes fees.
    /// Additionally, the invariant `buy_amount / sell_amount <= limit_xrate`
    /// holds, but in general the ratio of the two amounts does not equal the
    /// limit exchange rate.
    pub fn fill_order_at_price(
        &mut self,
        pair: TokenPair,
        limit_xrate: f64,
    ) -> Result<(f64, f64), OverlapError> {
        // NOTE: This method works by searching for the "best" counter
        // transitive orders, as such we need to search for transitive orders
        // in the inverse direction, and compute the maximum xrate that still
        // overlaps with the specified limit xrate.
        let inverse_pair = TokenPair {
            buy: pair.sell,
            sell: pair.buy,
        };
        let max_xrate = (1.0 / limit_xrate) / FEE_FACTOR;

        let mut total_buy_volume = 0.0;
        let mut total_sell_volume = 0.0;
        while let Some(flow) = self.fill_optimal_transitive_order_if(inverse_pair, |flow| {
            flow.exchange_rate <= max_xrate
        })? {
            total_buy_volume += flow.capacity / flow.exchange_rate;
            total_sell_volume += flow.capacity;
        }

        Ok((total_buy_volume, total_sell_volume))
    }

    /// Fills the optimal transitive order for the specified token pair. This
    /// method is similar to `Orderbook::fill_optimal_transitive_order_if`
    /// except it does not check a condition on the discovered path's flow
    /// before filling.
    fn fill_optimal_transitive_order(
        &mut self,
        pair: TokenPair,
    ) -> Result<Option<Flow>, OverlapError> {
        self.fill_optimal_transitive_order_if(pair, |_| true)
    }

    /// Fills the optimal transitive order (i.e. with the lowest exchange rate)
    /// for the specified token pair by pushing flow from the buy token to the
    /// sell token, if the condition is met. The trading path through the
    /// orderbook graph is filled to maximum capacity, reducing the remaining
    /// order amounts and user balances along the way, returning the flow for
    /// the path.
    ///
    /// Returns `None` if the condition is not met or there is no path between
    /// the token pair.
    fn fill_optimal_transitive_order_if(
        &mut self,
        pair: TokenPair,
        mut condition: impl FnMut(&Flow) -> bool,
    ) -> Result<Option<Flow>, OverlapError> {
        if !self.is_token_pair_valid(pair) {
            return Ok(None);
        }

        let (start, end) = (node_index(pair.buy), node_index(pair.sell));
        let predecessors = bellman_ford::search(&self.projection, start)?.1;
        let path = match path::find_path(&predecessors, start, end) {
            Some(path) => path,
            None => return Ok(None),
        };

        let (capacity, exchange_rate) =
            self.find_path_capacity_and_price(&path).unwrap_or_else(|| {
                panic!(
                    "failed to fill detected shortest path {}",
                    format_path(&path),
                )
            });

        let flow = Flow {
            path,
            exchange_rate,
            capacity,
        };
        if !condition(&flow) {
            return Ok(None);
        }

        self.fill_path_with_capacity(&flow.path, capacity)
            .unwrap_or_else(|| {
                panic!(
                    "failed to fill with capacity along detected path {}",
                    format_path(&flow.path),
                )
            });

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
        while let Some(true) = self
            .orders
            .best_order_for_pair(pair)
            .map(|order| order.get_effective_amount(&self.users) <= MIN_AMOUNT)
        {
            self.orders.remove_pair_order(pair);
        }

        let edge = self.get_pair_edge(pair).unwrap_or_else(|| {
            panic!(
                "missing edge between token pair {}->{} with orders",
                pair.buy, pair.sell
            )
        });
        if let Some(order) = self.orders.best_order_for_pair(pair) {
            self.projection[edge] = order.weight();
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

    /// Fills a trading path through the orderbook to maximum capacity, reducing
    /// the remaining order amounts and user balances along the way, returning
    /// the amount of flow that left the first node in the path and the final
    /// transitive price (i.e. the final price of the last node as a result of
    /// trading along the path) or `None` if the path was invalid.
    ///
    /// Note that currently, user buy token balances are not incremented as a
    /// result of filling orders along a path.
    fn fill_path(&mut self, path: &[NodeIndex]) -> Option<(f64, f64)> {
        let (capacity, price) = self.find_path_capacity_and_price(path)?;
        self.fill_path_with_capacity(path, capacity)
            .unwrap_or_else(|| {
                panic!(
                    "failed to fill with capacity along detected path {}",
                    format_path(path),
                )
            });

        Some((capacity, price))
    }

    /// Finds a transitive trade along a path and returns the maximum capacity
    /// of the path expressed in an amount of the starting token and the
    /// transitive price. Returns `None` if the path doesn't exist.
    fn find_path_capacity_and_price(&self, path: &[NodeIndex]) -> Option<(f64, f64)> {
        let mut capacity = f64::INFINITY;
        let mut transitive_price = 1.0;
        for pair in pairs_on_path(path) {
            let order = self.orders.best_order_for_pair(pair)?;
            transitive_price *= order.price;

            let sell_amount = order.get_effective_amount(&self.users);
            capacity = num::min(capacity, sell_amount * transitive_price);
        }

        Some((capacity, transitive_price))
    }

    /// Pushes flow through a path of orders reducing order amounts and user
    /// balances as well as updating the projection graph by updating the
    /// weights to reflect the new graph. Returns `None` if the path does not
    /// exist.
    ///
    /// Note that currently, user buy token balances are not incremented as a
    /// result of filling orders along a path.
    fn fill_path_with_capacity(&mut self, path: &[NodeIndex], capacity: f64) -> Option<()> {
        let mut transitive_price = 1.0;
        for pair in pairs_on_path(path) {
            let (order, user) = self.best_order_with_user_for_pair_mut(pair)?;

            transitive_price *= order.price;

            // NOTE: `capacity` here is a buy amount, so we need to divide by
            // the price to get the sell amount being filled.
            let fill_amount = capacity / transitive_price;

            order.amount -= fill_amount;
            debug_assert!(
                order.amount >= -num::max_rounding_error(fill_amount),
                "remaining amount underflow for order {}-{}",
                order.user,
                order.id,
            );

            let new_balance = user.deduct_from_balance(pair.sell, fill_amount);

            if new_balance < MIN_AMOUNT {
                user.clear_balance(pair.sell);
                self.update_projection_graph_node(pair.sell);
            } else if order.amount < MIN_AMOUNT {
                self.update_projection_graph_edge(pair);
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

/// Returns true if an auction element should be included in the price graph.
///
/// Currently auction elements are ignored if:
/// - They are "dust" orders, that is their remaining amount or balance is less
///   than the minimum amount that the exchange allows for trades
/// - They have a `0` price numerator or denominator
fn should_include_auction_element(element: &Element) -> bool {
    const MIN_AMOUNT_U128: u128 = MIN_AMOUNT as _;
    const MIN_AMOUNT_U256: U256 = U256([MIN_AMOUNT as _, 0, 0, 0]);

    let is_dust_order =
        element.remaining_sell_amount < MIN_AMOUNT_U128 || element.balance < MIN_AMOUNT_U256;
    let has_valid_price = element.price.numerator != 0 && element.price.denominator != 0;

    !is_dust_order && has_valid_price
}

/// Returns a new transitive order from an orderbook graph price and capacity.
fn transitive_order_from_capacity_and_price(
    capacity: f64,
    transitive_price: f64,
) -> TransitiveOrder {
    // NOTE: We now have the capacity and price for this transitive order which
    // needs to be converted to a buy and sell amount. We have:
    // - `price = FEE_FACTOR * buy_amount / sell_amount`
    // - `capacity = sell_amount * price`
    // Solving for `buy_amount` and `sell_amount`, we get:
    let buy = capacity / FEE_FACTOR;
    let sell = capacity / transitive_price;

    TransitiveOrder { buy, sell }
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
    use crate::encoding::UserId;
    use assert_approx_eq::assert_approx_eq;

    /// Returns a `UserId` for a test user index.
    ///
    /// This method is meant to be used in conjunction with orderbooks created
    /// with the `orderbook` macro.
    fn user_id(id: u8) -> UserId {
        UserId::repeat_byte(id)
    }

    /// Macro for constructing an orderbook using a DSL for testing purposes.
    macro_rules! orderbook {
        (
            users {$(
                @ $user:tt {$(
                    token $token:tt => $balance:expr,
                )*}
            )*}
            orders {$(
                owner @ $owner:tt
                buying $buy:tt [ $buy_amount:expr ]
                selling $sell:tt [ $sell_amount:expr ] $( ($remaining:expr) )?
            ,)*}
        ) => {{
            let mut balances = std::collections::HashMap::new();
            $($(
                balances.insert(($user, $token), primitive_types::U256::from($balance));
            )*)*
            let mut users = std::collections::HashMap::new();
            let elements = vec![$(
                $crate::encoding::Element {
                    user: user_id($owner),
                    balance: balances[&($owner, $sell)],
                    pair: $crate::encoding::TokenPair {
                        buy: $buy,
                        sell: $sell,
                    },
                    valid: $crate::encoding::Validity {
                        from: 0,
                        to: u32::max_value(),
                    },
                    price: $crate::encoding::Price {
                        numerator: $buy_amount,
                        denominator: $sell_amount,
                    },
                    remaining_sell_amount: match &[$sell_amount, $($remaining)?][..] {
                        [_, remaining] => *remaining,
                        _ => $sell_amount,
                    },
                    id: {
                        let count = users.entry($owner).or_insert(0u16);
                        let id = *count;
                        *count += 1;
                        id
                    },
                },
            )*];
            Orderbook::from_elements(elements)
        }};
    }

    impl Orderbook {
        /// Retrieve the weight of an edge in the projection graph. This is used for
        /// testing that the projection graph is in sync with the order map.
        fn get_projected_pair_weight(&self, pair: TokenPair) -> f64 {
            let edge = match self.get_pair_edge(pair) {
                Some(edge) => edge,
                None => return f64::INFINITY,
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
        let mut orderbook = orderbook! {
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

        orderbook.reduce_overlapping_orders();
        // NOTE: We expect user 1's 2->1 order to be completely filled as well
        // as user 2's 5->3 order and user 4's 6->7 order.
        assert_eq!(orderbook.num_orders(), 6);
        assert!(!orderbook.is_overlapping());
    }

    #[test]
    fn path_finding_operations_fail_on_overlapping_orders() {
        //  /---0.5---v
        // 0          1
        //  ^---0.5---/
        let mut orderbook = orderbook! {
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
        assert!(orderbook.fill_transitive_orders(pair, None).is_err());
        assert!(orderbook.fill_market_order(pair, 10_000_000.0).is_err());
        assert!(orderbook.fill_order_at_price(pair, 1.0).is_err());
        assert!(orderbook.fill_optimal_transitive_order(pair).is_err());
    }

    #[test]
    fn detects_overlapping_transitive_orders() {
        // 0 --1.0--> 1 --0.5--> 2 --1.0--> 3 --1.0--> 4
        //            ^---------1.0--------/^---0.5---/
        let mut orderbook = orderbook! {
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

        let overlap =
            orderbook.reduce_overlapping_transitive_orderbook(Market { base: 1, quote: 2 });

        // Transitive order `2 -> 3 -> 1` buying 2 selling 1
        assert_eq!(overlap.asks.len(), 1);
        assert_approx_eq!(overlap.asks[0].buy, 500_000.0);
        assert_approx_eq!(overlap.asks[0].sell, 500_000.0 / FEE_FACTOR);

        // Transitive order `1 -> 2` buying 1 selling 2
        assert_eq!(overlap.bids.len(), 1);
        assert_approx_eq!(overlap.bids[0].buy, 1_000_000.0);
        assert_approx_eq!(overlap.bids[0].sell, 2_000_000.0);
    }

    #[test]
    fn includes_transitive_order_only_once() {
        // /---0.5---v
        // 0         1
        // ^---1.0---/
        // ^---1.5--/
        let mut orderbook = orderbook! {
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

        let overlap =
            orderbook.reduce_overlapping_transitive_orderbook(Market { base: 0, quote: 1 });

        // Transitive order `1 -> 0` buying 1 selling 0
        assert_eq!(overlap.asks.len(), 1);
        assert_approx_eq!(overlap.asks[0].buy, 1_000_000.0);
        assert_approx_eq!(overlap.asks[0].sell, 1_000_000.0);

        // Transitive order `0 -> 1` buying 0 selling 1
        assert_eq!(overlap.bids.len(), 1);
        assert_approx_eq!(overlap.bids[0].buy, 50_000_000.0);
        assert_approx_eq!(overlap.bids[0].sell, 100_000_000.0);

        // NOTE: This is counter-intuitive, but there should only be one
        // transitive order from `1 -> 0` even if there are two orders that
        // overlap with the large `0 -> 1` order. This is because whenever a
        // negative cycle is found, it gets split into two transitive orders,
        // one from `base -> quote` (ask) and the other from `quote -> base`
        // (bid). These transitive orders are **completely** reduced, so even if
        // there are other orders that overlap with the remaining amount of one
        // of these transitive orders, they might not get found by
        // `reduce_overlapping_transitive_orderbook`. Rest assured, they will
        // get found by `fill_transitive_orderbook` and ultimately included in
        // the final transitive orderbook.
    }

    #[test]
    fn fills_transitive_orders_with_maximum_spread() {
        //    /--1.0--v
        //   /        v---2.0--\
        //  /---4.0---v         \
        // 1          2          3
        //  \                    ^
        //   \--------1.0-------/
        let mut orderbook = orderbook! {
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
        let pair = TokenPair { buy: 1, sell: 2 };

        let orders = orderbook
            .clone()
            .fill_transitive_orders(pair, Some(0.5))
            .unwrap();
        assert_eq!(orders.len(), 1);
        assert_approx_eq!(orders[0].buy, 1_000_000.0);
        assert_approx_eq!(orders[0].sell, 1_000_000.0);

        let orders = orderbook
            .clone()
            .fill_transitive_orders(pair, Some(1.0))
            .unwrap();
        assert_eq!(orders.len(), 1);

        let orders = orderbook
            .clone()
            .fill_transitive_orders(pair, Some((2.0 * FEE_FACTOR) - 1.0))
            .unwrap();
        assert_eq!(orders.len(), 2);
        assert_approx_eq!(orders[1].buy, 1_000_000.0);
        assert_approx_eq!(orders[1].sell, 500_000.0 / FEE_FACTOR);

        let orders = orderbook.fill_transitive_orders(pair, Some(3.0)).unwrap();
        assert_eq!(orders.len(), 3);
        assert_approx_eq!(orders[2].buy, 4_000_000.0);
        assert_approx_eq!(orders[2].sell, 1_000_000.0);
    }

    #[test]
    fn fills_all_transitive_orders_without_maximum_spread() {
        //    /--1.0--v
        //   /        v---2.0--\
        //  /---4.0---v         \
        // 1          2          3
        //  \                    ^
        //   \--------1.0-------/
        let mut orderbook = orderbook! {
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
        let pair = TokenPair { buy: 1, sell: 2 };

        let orders = orderbook.fill_transitive_orders(pair, None).unwrap();
        assert_eq!(orders.len(), 3);

        assert_approx_eq!(orders[0].buy, 1_000_000.0);
        assert_approx_eq!(orders[0].sell, 1_000_000.0);

        assert_approx_eq!(orders[1].buy, 1_000_000.0);
        assert_approx_eq!(orders[1].sell, 500_000.0 / FEE_FACTOR);

        assert_approx_eq!(orders[2].buy, 4_000_000.0);
        assert_approx_eq!(orders[2].sell, 1_000_000.0);
    }

    #[test]
    fn fills_market_order_with_correct_price() {
        //    /-101.0--v
        //   /--105.0--v
        //  /---111.0--v
        // 1           2
        // ^--.0101---/
        // ^--.0105--/
        // ^--.0110-/
        let mut orderbook = orderbook! {
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

        assert!(!orderbook.is_overlapping());

        assert_approx_eq!(
            orderbook
                .clone()
                .fill_market_order(TokenPair { buy: 2, sell: 1 }, 500_000.0)
                .unwrap()
                .unwrap(),
            99.0 / FEE_FACTOR.powi(2)
        );
        assert_approx_eq!(
            orderbook
                .clone()
                .fill_market_order(TokenPair { buy: 1, sell: 2 }, 50_000_000.0)
                .unwrap()
                .unwrap(),
            1.0 / (101.0 * FEE_FACTOR.powi(2))
        );

        assert_approx_eq!(
            orderbook
                .clone()
                .fill_market_order(TokenPair { buy: 2, sell: 1 }, 1_500_000.0)
                .unwrap()
                .unwrap(),
            95.0 / FEE_FACTOR.powi(2)
        );
        assert_approx_eq!(
            orderbook
                .clone()
                .fill_market_order(TokenPair { buy: 1, sell: 2 }, 150_000_000.0)
                .unwrap()
                .unwrap(),
            1.0 / (105.0 * FEE_FACTOR.powi(2))
        );

        assert_approx_eq!(
            orderbook
                .clone()
                .fill_market_order(TokenPair { buy: 2, sell: 1 }, 2_500_000.0)
                .unwrap()
                .unwrap(),
            90.0 / FEE_FACTOR.powi(2)
        );
        assert_approx_eq!(
            orderbook
                .clone()
                .fill_market_order(TokenPair { buy: 1, sell: 2 }, 250_000_000.0)
                .unwrap()
                .unwrap(),
            1.0 / (110.0 * FEE_FACTOR.powi(2))
        );

        let price = orderbook
            .fill_market_order(TokenPair { buy: 2, sell: 1 }, 10_000_000.0)
            .unwrap();
        assert!(price.is_none());

        let price = orderbook
            .fill_market_order(TokenPair { buy: 1, sell: 2 }, 1_000_000_000.0)
            .unwrap();
        assert!(price.is_none());

        assert_eq!(orderbook.num_orders(), 0);
    }

    #[test]
    fn fills_order_at_price_with_correct_amount() {
        //    /-1.0---v
        //   /--2.0---v
        //  /---4.0---v
        // 1          2
        let mut orderbook = orderbook! {
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

        let (buy, sell) = orderbook
            .clone()
            // NOTE: 1 for 1.001 is not enough to match any volume because
            // fees need to be applied twice!
            .fill_order_at_price(TokenPair { buy: 2, sell: 1 }, 1.0 / FEE_FACTOR)
            .unwrap();
        assert_approx_eq!(buy, 0.0);
        assert_approx_eq!(sell, 0.0);

        let (buy, sell) = orderbook
            .clone()
            .fill_order_at_price(TokenPair { buy: 2, sell: 1 }, 1.0 / FEE_FACTOR.powi(2))
            .unwrap();
        assert_approx_eq!(buy, 1_000_000.0);
        assert_approx_eq!(sell, 1_000_000.0 * FEE_FACTOR);

        let (buy, sell) = orderbook
            .clone()
            .fill_order_at_price(TokenPair { buy: 2, sell: 1 }, 0.3)
            .unwrap();
        assert_approx_eq!(buy, 2_000_000.0);
        assert_approx_eq!(sell, 3_000_000.0 * FEE_FACTOR);

        let (buy, sell) = orderbook
            .clone()
            .fill_order_at_price(TokenPair { buy: 2, sell: 1 }, 0.25 / FEE_FACTOR.powi(2))
            .unwrap();
        assert_approx_eq!(buy, 3_000_000.0);
        assert_approx_eq!(sell, 7_000_000.0 * FEE_FACTOR);

        let (buy, sell) = orderbook
            .fill_order_at_price(TokenPair { buy: 2, sell: 1 }, 0.1)
            .unwrap();
        assert_approx_eq!(buy, 3_000_000.0);
        assert_approx_eq!(sell, 7_000_000.0 * FEE_FACTOR);
    }

    #[test]
    fn removes_dust_orders() {
        let mut orderbook = orderbook! {
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
        assert_approx_eq!(order.weight(), orderbook.get_projected_pair_weight(pair));

        orderbook.reduce_overlapping_orders();
        let order = orderbook.orders.best_order_for_pair(pair);
        assert_eq!(orderbook.num_orders(), 1);
        assert!(order.is_none());
        assert!(orderbook.get_projected_pair_weight(pair).is_infinite());
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

        let (capacity, transitive_price) = orderbook.find_path_capacity_and_price(&path).unwrap();
        assert_approx_eq!(
            capacity,
            // NOTE: We can send a little more than the balance limit of user 2
            // because some of it gets eaten up by the fees along the way.
            500_000.0 * FEE_FACTOR.powi(2)
        );
        assert_approx_eq!(transitive_price, 0.5 * FEE_FACTOR.powi(4));

        let filled = orderbook.fill_path(&path).unwrap();
        assert_eq!(filled, (capacity, transitive_price));

        // NOTE: The expected fill amounts are:
        //  Order | buy amt | sell amt | xrate
        // -------+---------+----------+-------
        // 0 -> 1 | 501_000 |  500_500 |   1.0
        // 1 -> 2 | 500_500 |  500_000 |   1.0
        // 2 -> 3 | 500_000 |  499_500 |   1.0
        // 3 -> 4 | 499_500 |  998_000 |   0.5

        assert_approx_eq!(
            orderbook.get_projected_pair_weight(TokenPair { buy: 1, sell: 2 }),
            // NOTE: user 2's order has no more balance so expect the new weight
            // between tokens 1 and 2 to be user 5's order with the worse price
            // of 2:1 (meaning it needs twice as much token 1 to get the same
            // amount of token 2 when pushing flow through that edge).
            (2.0 * FEE_FACTOR).log2()
        );
        // NOTE: User 2's other order selling token 2 for 4 also has no more
        // balance so check that it was also removed from the order map and from
        // the projection graph.
        assert!(orderbook
            .orders
            .best_order_for_pair(TokenPair { buy: 4, sell: 2 })
            .is_none());
        assert!(orderbook
            .get_projected_pair_weight(TokenPair { buy: 4, sell: 2 })
            .is_infinite());
        // NOTE: User 3's order selling token 3 for 2 has become a dust order
        // (since it has a remaining amount of about 500), check that it was
        // removed.
        assert!(orderbook
            .orders
            .best_order_for_pair(TokenPair { buy: 2, sell: 3 })
            .is_none());
        assert!(orderbook
            .get_projected_pair_weight(TokenPair { buy: 2, sell: 3 })
            .is_infinite());

        assert_eq!(orderbook.num_orders(), 3);

        let transitive_price_0_1 = FEE_FACTOR;
        assert_approx_eq!(
            orderbook
                .orders
                .best_order_for_pair(TokenPair { buy: 0, sell: 1 })
                .unwrap()
                .amount,
            1_000_000.0 - capacity / transitive_price_0_1
        );
        assert_approx_eq!(
            orderbook.users[&user_id(1)].balance_of(1),
            1_000_000.0 - capacity / transitive_price_0_1
        );

        assert_approx_eq!(
            orderbook
                .orders
                .best_order_for_pair(TokenPair { buy: 1, sell: 2 })
                .unwrap()
                .amount,
            1_000_000.0
        );
        assert_approx_eq!(orderbook.users[&user_id(5)].balance_of(2), 1_000_000.0);

        assert_approx_eq!(
            orderbook
                .orders
                .best_order_for_pair(TokenPair { buy: 3, sell: 4 })
                .unwrap()
                .amount,
            2_000_000.0 - capacity / transitive_price
        );
        assert_approx_eq!(
            orderbook.users[&user_id(4)].balance_of(4),
            10_000_000.0 - capacity / transitive_price
        );
    }

    #[test]
    fn fill_market_order_returns_none_for_invalid_token_pairs() {
        //   /---1.0---v
        //  0          1          2 --0.5--> 4
        //  ^---1.0---/
        let mut orderbook = orderbook! {
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
            orderbook
                .fill_market_order(TokenPair { buy: 1, sell: 3 }, 500_000.0)
                .unwrap(),
            None,
        );
        // Tokens 4 and 1 are not connected.
        assert_eq!(
            orderbook
                .fill_market_order(TokenPair { buy: 4, sell: 1 }, 500_000.0)
                .unwrap(),
            None,
        );
        // Tokens 5 and 42 are out of bounds.
        assert_eq!(
            orderbook
                .fill_market_order(TokenPair { buy: 5, sell: 1 }, 500_000.0)
                .unwrap(),
            None,
        );
        assert_eq!(
            orderbook
                .fill_market_order(TokenPair { buy: 2, sell: 42 }, 500_000.0)
                .unwrap(),
            None,
        );
    }

    #[test]
    fn fuzz_calculates_rounding_errors_based_on_amounts() {
        // NOTE: Discovered by fuzzer, see
        // https://github.com/gnosis/dex-services/issues/916#issuecomment-634457245

        let mut orderbook = orderbook! {
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

        let (buy, sell) = orderbook
            .fill_order_at_price(TokenPair { buy: 1, sell: 0 }, 1.0)
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
