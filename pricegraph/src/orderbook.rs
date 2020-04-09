//! Implementation of a graph representation of an orderbook where tokens are
//! vertices and orders are edges (with users and balances as auxiliary data
//! to these edges).
//!
//! Storage is optimized for graph-related operations such as listing the edges
//! (i.e. orders) connecting a token pair.

mod order;
mod user;

use self::order::{Order, OrderCollector, OrderMap};
use self::user::UserMap;
use crate::encoding::{Element, InvalidLength, TokenId, TokenPair};
use crate::graph::bellman_ford;
use petgraph::graph::{DiGraph, EdgeIndex, NodeIndex};
use std::cmp;
use std::f64;

/// A graph representation of a complete orderbook.
#[derive(Debug)]
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
    /// Reads an orderbook from encoded bytes returning an error if the encoded
    /// orders are invalid.
    pub fn read(bytes: impl AsRef<[u8]>) -> Result<Orderbook, InvalidLength> {
        let elements = Element::read_all(bytes.as_ref())?;
        Ok(Orderbook::from_elements(elements))
    }

    /// Creates an orderbook from an iterator over decoded auction elements.
    fn from_elements(elements: impl IntoIterator<Item = Element>) -> Orderbook {
        let mut max_token = 0;
        let mut orders = OrderCollector::default();
        let mut users = UserMap::default();

        for element in elements {
            let TokenPair { buy, sell } = element.pair;
            let order_id = users
                .entry(element.user)
                .or_default()
                .include_order(&element);

            max_token = cmp::max(max_token, cmp::max(buy, sell));
            orders.insert_order(Order::new(element, order_id));
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

    /// Detects whether or not a solution can be found by finding negative
    /// cycles in the projection graph starting from the fee token.
    ///
    /// Conceptually, a negative cycle is a trading path starting and ending at
    /// a token (going through an arbitrary number of other distinct tokens)
    /// where the total weight is less than `0`, i.e. the effective sell price
    /// is less than `1`. This means that there is a price overlap along this
    /// ring trade and it is connected to the fee token.
    pub fn is_overlapping(&self) -> bool {
        let fee_token = node_index(0);
        bellman_ford::search(&self.projection, fee_token).is_err()
    }

    /// Updates the projection graph for every token pair.
    pub fn update_projection_graph(&mut self) {
        let pairs = self
            .orders
            .all_pairs()
            .map(|(pair, _)| pair)
            .collect::<Vec<_>>();
        for pair in pairs {
            self.update_projection_graph_edge(pair);
        }
    }

    /// Updates the projection graph edge between a token pair.
    ///
    /// This is done by removing all "dust" orders, i.e. orders whose remaining
    /// amount or whose users remaining balance is zero, and then either
    /// updating the projection graph edge with the weight of the new cheapest
    /// order or removing the edge entirely if no orders remain for the given
    /// token pair.
    fn update_projection_graph_edge(&mut self, pair: TokenPair) {
        while let Some(true) = self
            .orders
            .pair_order(pair)
            .map(|order| order.get_effective_amount(&self.users) <= 0.0)
        {
            self.orders.remove_pair_order(pair);
        }

        let edge = self
            .get_pair_edge(pair)
            .expect("missing edge between token pair with orders");
        if let Some(order) = self.orders.pair_order(pair) {
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

    /// Retrieve the weight of an edge in the projection graph. This is used for
    /// testing that the projection graph is in sync with the order map.
    #[cfg(test)]
    fn get_projected_pair_weight(&self, pair: TokenPair) -> f64 {
        let edge = self.get_pair_edge(pair).unwrap();
        self.projection[edge]
    }
}

/// Create a node index from a token ID.
fn node_index(token: TokenId) -> NodeIndex {
    NodeIndex::new(token.into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data;
    use crate::encoding::UserId;
    use crate::num;

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
                },
            )*];
            Orderbook::from_elements(elements)
        }};
    }

    #[test]
    fn reads_real_orderbook() {
        let orderbook = data::read_default_orderbook();
        assert_eq!(orderbook.num_orders(), 896);
        assert!(orderbook.is_overlapping());
    }

    #[test]
    fn removes_drained_and_balanceless_orders() {
        let mut orderbook = orderbook! {
            users {
                @1 {
                    token 0 => 5_000_000,
                }
                @2 {
                    token 0 => 1_000_000_000,
                }
                @3 {
                    token 0 => 0,
                }
            }
            orders {
                owner @1 buying 1 [1_000_000_000] selling 0 [1_000_000_000],
                owner @2 buying 1 [1_000_000_000] selling 0 [2_000_000_000] (0),
                owner @3 buying 1 [1_000_000_000] selling 0 [3_000_000_000],
            }
        };

        let pair = TokenPair { buy: 1, sell: 0 };

        let order = orderbook.orders.pair_order(pair).unwrap();
        assert_eq!(orderbook.num_orders(), 3);
        assert_eq!(order.user, user_id(3));
        assert!(num::eq(
            order.weight(),
            orderbook.get_projected_pair_weight(pair),
        ));

        orderbook.update_projection_graph();
        let order = orderbook.orders.pair_order(pair).unwrap();
        assert_eq!(orderbook.num_orders(), 1);
        assert_eq!(order.user, user_id(1));
        assert!(num::eq(
            order.weight(),
            orderbook.get_projected_pair_weight(pair),
        ));
    }
}
