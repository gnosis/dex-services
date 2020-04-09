//! Implementation of a graph representation of an orderbook where tokens are
//! vertices and orders are edges (with users and balances as auxiliary data
//! to these edges).
//!
//! Storage is optimized for graph-related operations such as listing the edges
//! (i.e. orders) connecting a token pair.

use crate::encoding::{Element, InvalidLength, Price, TokenId, TokenPair, UserId};
use crate::graph::bellman_ford;
use petgraph::graph::{DiGraph, NodeIndex};
use primitive_types::U256;
use std::cmp;
use std::collections::HashMap;
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
        let mut max_token = 0;
        let mut orders = OrderMap::default();
        let mut users = UserMap::default();

        for element in Element::read_all(bytes.as_ref())? {
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

        Ok(Orderbook {
            orders,
            users,
            projection,
        })
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

/// Type definition for a mapping of orders between buy and sell tokens.
#[derive(Debug, Default)]
struct OrderMap(HashMap<TokenId, HashMap<TokenId, Vec<Order>>>);

impl OrderMap {
    /// Adds a new order to the order map.
    fn insert_order(&mut self, order: Order) {
        self.0
            .entry(order.pair.sell)
            .or_default()
            .entry(order.pair.buy)
            .or_default()
            .push(order);
    }

    /// Returns an iterator over the collection of orders for each token pair.
    fn all_pairs(&self) -> impl Iterator<Item = (TokenPair, &'_ [Order])> + '_ {
        self.0.iter().flat_map(|(&sell, o)| {
            o.iter()
                .map(move |(&buy, o)| (TokenPair { sell, buy }, o.as_slice()))
        })
    }

    /// Returns an iterator over the collection of orders for each token pair.
    fn all_pairs_mut(&mut self) -> impl Iterator<Item = (TokenPair, &'_ mut Vec<Order>)> + '_ {
        self.0.iter_mut().flat_map(|(&sell, o)| {
            o.iter_mut()
                .map(move |(&buy, o)| (TokenPair { sell, buy }, o))
        })
    }
}

/// A single order with a reference to the user.
///
/// Note that we approximate amounts and prices with floating point numbers.
/// While this can lead to rounding errors it greatly simplifies the graph
/// computations and still leads to acceptable estimates.
#[derive(Debug, PartialEq)]
struct Order {
    /// The user owning the order.
    user: UserId,
    /// The index of an order per user.
    index: usize,
    /// The token pair.
    pair: TokenPair,
    /// The maximum capacity for this order, this is equivalent to the order's
    /// sell amount (or price denominator). Note that orders are also limited by
    /// their user's balance.
    amount: f64,
    /// The effective sell price for this order, that is price for this order
    /// including fees of the sell token expressed in the buy token.
    ///
    /// Specifically, this is the sell amount over the buy amount or price
    /// denominator over price numerator, which is the inverse price as
    /// expressed by the exchange.
    price: f64,
}

impl Order {
    /// Creates a new order from an ID and an orderbook element.
    fn new(element: Element, index: usize) -> Self {
        let amount = if is_unbounded(&element) {
            f64::INFINITY
        } else {
            element.price.denominator as _
        };
        let price = as_effective_sell_price(&element.price);

        Order {
            user: element.user,
            index,
            pair: element.pair,
            amount,
            price,
        }
    }

    /// Compare two orders by descending price order.
    ///
    /// This method is used for sorting orders, instead of just sorting by key
    /// on `price` field because `f64`s don't implement `Ord` and as such cannot
    /// be used for sorting. This be because there is no real ordering for
    /// `NaN`s and `NaN < 0 == false` and `NaN >= 0 == false` (cf. IEEE 754-2008
    /// section 5.11), which can cause serious problems with sorting.
    fn cmp_descending_prices(a: &Order, b: &Order) -> cmp::Ordering {
        b.price
            .partial_cmp(&a.price)
            .expect("orders cannot have NaN prices")
    }

    /// The weight of the order in the graph. This is the base-2 logarithm of
    /// the price including fees. This enables transitive prices to be computed
    /// using addition.
    pub fn weight(&self) -> f64 {
        self.price.log2()
    }
}

/// Returns `true` if the order is unbounded, that is it has an unlimited sell
/// amount.
fn is_unbounded(element: &Element) -> bool {
    const UNBOUNDED_AMOUNT: u128 = u128::max_value();
    element.price.numerator == UNBOUNDED_AMOUNT || element.price.denominator == UNBOUNDED_AMOUNT
}

/// Calculates an effective price as a `f64` from a price fraction.
fn as_effective_sell_price(price: &Price) -> f64 {
    const FEE_FACTOR: f64 = 1.0 / 0.999;
    FEE_FACTOR * (price.denominator as f64) / (price.numerator as f64)
}

/// A type definiton for a mapping between user IDs to user data.
type UserMap = HashMap<UserId, User>;

/// User data containing balances and number of orders.
#[derive(Debug, Default, PartialEq)]
struct User {
    /// User balances per token.
    balances: HashMap<TokenId, f64>,
    /// The number of orders this user has.
    num_orders: usize,
}

impl User {
    /// Adds an encoded orderbook element to the user data, including the
    /// reported balance and incrementing the number of orders.
    fn include_order(&mut self, element: &Element) -> usize {
        let order_id = self.num_orders;

        self.balances
            .entry(element.pair.sell)
            .or_insert_with(|| u256_to_f64(element.balance));
        self.num_orders += 1;

        order_id
    }
}

/// Convert an unsigned 256-bit integer into a `f64`.
fn u256_to_f64(u: U256) -> f64 {
    let (u, factor) = match u {
        U256([_, _, 0, 0]) => (u, 1.0),
        U256([_, _, _, 0]) => (u >> 64, 2.0f64.powi(64)),
        U256([_, _, _, _]) => (u >> 128, 2.0f64.powi(128)),
    };
    (u.low_u128() as f64) * factor
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
