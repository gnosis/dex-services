//! Implementation of a graph representation of an orderbook where tokens are
//! vertices and orders are edges (with users and balances as auxiliary data
//! to these edges).
//!
//! Storage is optimized for graph-related operations such as listing the edges
//! (i.e. orders) connecting a token pair.

use crate::encoding::{Element, InvalidLength, Price, TokenId, TokenPair, UserId};
use crate::graph::bellman_ford;
use petgraph::graph::DiGraph;
use std::cmp;
use std::collections::HashMap;
use std::f64;

/// A graph representation of a complete orderbook.
#[cfg_attr(test, derive(Debug))]
pub struct Orderbook {
    /// A map of sell tokens to a mapping of buy tokens to orders such that
    /// `orders[sell][buy]` is a vector of orders selling token `sell` and
    /// buying token `buy`.
    orders: OrderMap,
    /// A projection of the order book onto a graph of lowest priced orders
    /// between tokens.
    projection: DiGraph<TokenId, f64>,
}

impl Orderbook {
    /// Reads an orderbook from encoded bytes returning an error if the encoded
    /// orders are invalid.
    pub fn read(bytes: impl AsRef<[u8]>) -> Result<Orderbook, InvalidLength> {
        let mut max_token = 0;
        let mut orders = OrderMap::new();

        for element in Element::read_all(bytes.as_ref())? {
            let TokenPair { buy, sell } = element.pair;
            max_token = cmp::max(max_token, cmp::max(buy, sell));
            orders
                .entry(sell)
                .or_default()
                .entry(buy)
                .or_default()
                .push(element.into());
        }

        // NOTE: Sort the orders per token pair in descending order, this makes
        // it so the last order is the cheapest one and can be determined given
        // a token pair in `O(1)`, and it can simply be `pop`-ed once its amount
        // is used up and removed from the graph in `O(1)` as well.
        for pair_orders in orders.values_mut().flat_map(|o| o.values_mut()) {
            pair_orders.sort_unstable_by(Order::cmp_descending_prices);
        }

        let mut projection = DiGraph::new();
        let token_nodes = (0..=max_token)
            .map(|token_id| projection.add_node(token_id))
            .collect::<Vec<_>>();
        projection.extend_with_edges(orders.iter().flat_map(|(sell, orders)| {
            let sell_node = token_nodes[*sell as usize];
            orders.iter().map({
                let token_nodes = &token_nodes;
                move |(buy, orders)| {
                    let buy_node = token_nodes[*buy as usize];
                    let weight = orders
                        .last()
                        .expect("unexpected empty pair orders collection in map")
                        .weight();
                    (sell_node, buy_node, weight)
                }
            })
        }));

        Ok(Orderbook { orders, projection })
    }

    /// Returns the number of orders in the orderbook.
    pub fn num_orders(&self) -> usize {
        self.orders
            .values()
            .flat_map(|o| o.values())
            .map(Vec::len)
            .sum()
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
        let fee_token = match self
            .projection
            .node_indices()
            .find(|i| self.projection[*i] == 0)
        {
            Some(token) => token,
            None => {
                // NOTE: The fee token is not in the graph, this means there are
                // no orders buying or selling the fee token and therefore it
                // cannot be overlapping.
                return false;
            }
        };

        bellman_ford::search(&self.projection, fee_token).is_err()
    }
}

/// Type definition for a mapping of orders between buy and sell tokens.
type OrderMap = HashMap<TokenId, HashMap<TokenId, Vec<Order>>>;

/// A single order with a reference to the user.
///
/// Note that we approximate amounts and prices with floating point numbers.
/// While this can lead to rounding errors it greatly simplifies the graph
/// computations and still leads to acceptable estimates.
#[cfg_attr(test, derive(Debug, PartialEq))]
struct Order {
    /// The user owning the order.
    pub user: UserId,
    /// The maximum capacity for this order, this is equivalent to the order's
    /// sell amount (or price denominator). Note that orders are also limited by
    /// their user's balance.
    pub amount: f64,
    /// The effective sell price for this order, that is price for this order
    /// including fees of the sell token expressed in the buy token.
    ///
    /// Specifically, this is the sell amount over the buy amount or price
    /// denominator over price numerator, which is the inverse price as
    /// expressed by the exchange.
    pub price: f64,
}

impl Order {
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

impl From<Element> for Order {
    fn from(element: Element) -> Self {
        let amount = if is_unbounded(&element) {
            f64::INFINITY
        } else {
            element.price.denominator as _
        };
        let price = as_effective_sell_price(&element.price);

        Order {
            user: element.user,
            amount,
            price,
        }
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
