//! Implementation of a graph representation of an orderbook where tokens are
//! vertices and orders are edges (with users and balances as auxiliary data
//! to these edges).
//!
//! Storage is optimized for graph-related operations such as listing the edges
//! (i.e. orders) connecting a token pair.

mod order;
mod user;

use self::order::{Order, OrderMap};
use self::user::UserMap;
use crate::encoding::{Element, InvalidLength, TokenId, TokenPair};
use crate::graph::bellman_ford;
use petgraph::graph::{DiGraph, NodeIndex};
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
        let mut orders = OrderMap::default();
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

        // NOTE: Sort the orders per token pair in descending order, this makes
        // it so the last order is the cheapest one and can be determined given
        // a token pair in `O(1)`, and it can simply be `pop`-ed once its amount
        // is used up and removed from the graph in `O(1)` as well.
        for (_, pair_orders) in orders.all_pairs_mut() {
            pair_orders.sort_unstable_by(Order::cmp_descending_prices);
        }

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
                    node_index(pair.sell),
                    node_index(pair.buy),
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
}

/// Create a node index from a token ID.
fn node_index(token: TokenId) -> NodeIndex {
    NodeIndex::new(token.into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use data_encoding::Specification;

    #[test]
    fn reads_real_orderbook() {
        let hex = {
            let mut spec = Specification::new();
            spec.symbols.push_str("0123456789abcdef");
            spec.ignore.push_str(" \n");
            spec.encoding().unwrap()
        };
        let encoded_orderbook = hex
            .decode(include_bytes!("../data/orderbook-5287195.hex"))
            .expect("orderbook contains invalid hex");

        let orderbook = Orderbook::read(&encoded_orderbook).expect("error reading orderbook");
        assert_eq!(orderbook.num_orders(), 896);
        assert!(orderbook.is_overlapping());
    }
}
