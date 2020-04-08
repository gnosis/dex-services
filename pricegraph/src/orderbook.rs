//! Implementation of a graph representation of an orderbook where tokens are
//! vertices and orders are edges (with users and balances as auxiliary data
//! to these edges).
//!
//! Storage is optimized for graph-related operations such as listing the edges
//! (i.e. orders) connecting a token pair.

use crate::encoding::{Element, InvalidLength, Price, TokenId, TokenPair, UserId};
use crate::graph::bellman_ford::{self, NegativeCycle};
use petgraph::graph::{DiGraph, NodeIndex};
use primitive_types::U256;
use std::cmp;
use std::collections::{hash_map, HashMap};
use std::f64;

/// The minimum trading amount as defined by the smart contract.
const MIN_AMOUNT: f64 = 10000.0;

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
            pair_orders.sort_unstable_by(Order::cmp_decending_prices);
        }

        let mut projection = DiGraph::new();
        for token_id in 0..=max_token {
            let token_node = projection.add_node(token_id);

            // NOTE: Tokens are added in order such that token_id == token_node
            // index, assert that the node index is indeed what we expect it to
            // be.
            debug_assert_eq!(token_node, node_index(token_id));
        }
        projection.extend_with_edges(orders.all_pairs().filter_map({
            |(pair, orders)| {
                let cheapest_order = orders.last()?;
                Some((
                    node_index(pair.sell),
                    node_index(pair.buy),
                    cheapest_order.weight(),
                ))
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

    /// Reduces an orderbook by filling ring trades along all negative cycles.
    ///
    /// Returns a vector with paths for all overlapping ring trades that are
    /// connected to the fee token, removing them from `self`.
    ///
    /// Conceptually, a negative cycle is a trading path starting and ending at
    /// a token (going through an arbitrary number of other distinct tokens)
    /// where the total weight is less than `0`, i.e. the effective sell price
    /// is less than `1`. This means that there is a price overlap along this
    /// ring trade. Since negative cycles are detected starting from token 0,
    /// then the discovered negative cycles additionally are connected to the
    /// fee token.
    pub fn reduce(&mut self) -> Vec<Vec<TokenId>> {
        // NOTE: First update the projection graph edges and removing all dust
        // and unfillable orders.
        self.update_projection_graph();

        let mut paths = Vec::new();

        let fee_token = node_index(0);
        while let Err(NegativeCycle(path)) = bellman_ford::search(&self.projection, fee_token) {
            if self.fill_path(&path) {
                paths.push(path.into_iter().map(|node| node.index() as _).collect());
            }
        }

        paths
    }

    /// Updates the projection graph for every token pair.
    fn update_projection_graph(&mut self) {
        let pairs = self
            .orders
            .all_pairs()
            .map(|(pair, _)| pair)
            .collect::<Vec<_>>();
        for pair in pairs {
            self.update_projection_graph_edge(pair);
        }
    }

    /// Updates the projection graph edge between a token pair. This is done by
    /// removing all filled and "dust" orders (i.e. orders whose effective
    /// amount is less than the `MIN_AMOUNT` threshold) and then either updates
    /// the projection graph edge with the weight of the new cheapest order or
    /// removes the edge entirely if no orders remain for the given token pair.
    fn update_projection_graph_edge(&mut self, pair: TokenPair) {
        let Self {
            ref mut orders,
            users,
            ref mut projection,
            ..
        } = self;

        let pair_orders = match orders.pair_orders_mut(pair) {
            Some(orders) => orders,
            None => return,
        };

        // NOTE: The orderbook structure ensures that `OrderMap` entries only
        // exist when there is at least one order between the token pair.
        debug_assert!(!pair_orders.is_empty(), "encountered empty OrderMap entry");

        // NOTE: Orders are sorted, so the last order is always the one with
        // the smallest price.
        while pair_orders
            .last()
            .map(|order| order.get_effective_amount(&users) < MIN_AMOUNT)
            == Some(true)
        {
            pair_orders.pop();
        }

        let (sell, buy) = (node_index(pair.sell), node_index(pair.buy));
        let edge = projection
            .find_edge(sell, buy)
            .expect("missing edge between token pair with orders");

        if let Some(order) = pair_orders.last() {
            projection[edge] = order.weight;
        } else {
            // No remaining orders, the edge can be removed from the order map
            // and the projection graph.
            orders.remove_pair(pair);
            projection.remove_edge(edge);
        }
    }

    /// Fills orders along a path updating order remaining amounts as well as
    /// deducting from user balances. Returns `true` if all filled amounts are
    /// above the minimum `MIN_AMOUNT` threadhold.
    ///
    /// Note that we do not increase the balance of user's buy token for orders
    /// as this can lead to overlapping pairs trading with eachother
    /// indefinitely.
    fn fill_path(&mut self, path: &[NodeIndex]) -> bool {
        let Self {
            ref mut orders,
            users,
            ..
        } = self;

        // First determine the capacity of the path expressed in the first and
        // final token of the path.
        let mut capacity = f64::INFINITY;
        let mut transient_price = 1.0;
        for pair in pairs_on_path(path) {
            let order = orders.cheapest_order(pair).expect("missing order on path");
            capacity = min(
                capacity,
                order.get_effective_amount(users) / transient_price,
            );
            transient_price *= order.price;
        }

        // Now that the capacity of the path has been determined, push the flow
        // around the orderbook deducting from the order amounts and user
        // balances.
        let mut transient_price = 1.0;
        let mut smallest_fill_amount = f64::INFINITY;
        for pair in pairs_on_path(path) {
            let order = orders
                .cheapest_order_mut(pair)
                .expect("missing order on path");
            let fill_amount = capacity / transient_price;
            order.amount -= fill_amount;
            users
                .get_mut(&order.user)
                .expect("missing user with order")
                .deduct_from_balance(order.pair.sell, fill_amount);
            transient_price *= order.price;
            smallest_fill_amount = min(smallest_fill_amount, fill_amount);
        }

        // Finally, update the projected graph considering the newly filled
        // orders.
        for pair in pairs_on_path(path) {
            self.update_projection_graph_edge(pair);
        }

        smallest_fill_amount >= MIN_AMOUNT
    }
}

/// Create a node index from a token ID.
fn node_index(token: TokenId) -> NodeIndex {
    NodeIndex::new(token.into())
}

/// Returns an iterator with all pairs on a path.
fn pairs_on_path(path: &[NodeIndex]) -> impl Iterator<Item = TokenPair> + '_ {
    path.windows(2).map(|segment| TokenPair {
        sell: segment[0].index() as _,
        buy: segment[1].index() as _,
    })
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

    /// Returns the orders for an order pair. Returns `None` if that pair has
    /// no orders.
    fn pair_orders(&self, pair: TokenPair) -> Option<&'_ [Order]> {
        Some(self.0.get(&pair.sell)?.get(&pair.buy)?.as_slice())
    }

    /// Returns a mutable reference to the orders for an order pair. Returns
    /// `None` if that pair has no orders.
    fn pair_orders_mut(&mut self, pair: TokenPair) -> Option<&'_ mut Vec<Order>> {
        self.0.get_mut(&pair.sell)?.get_mut(&pair.buy)
    }

    /// Returns a reference to the cheapest order given an order pair.
    fn cheapest_order(&self, pair: TokenPair) -> Option<&'_ Order> {
        self.pair_orders(pair)?.last()
    }

    /// Returns a mutable reference to the cheapest order given an order pair.
    fn cheapest_order_mut(&mut self, pair: TokenPair) -> Option<&'_ mut Order> {
        self.pair_orders_mut(pair)?.last_mut()
    }

    /// Removes a token pair and orders from the mapping.
    fn remove_pair(&mut self, pair: TokenPair) -> Option<Vec<Order>> {
        let sell_orders = self.0.get_mut(&pair.sell)?;
        let removed = sell_orders.remove(&pair.buy)?;
        if sell_orders.is_empty() {
            self.0.remove(&pair.sell);
        }

        Some(removed)
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
    fn cmp_decending_prices(a: &Order, b: &Order) -> cmp::Ordering {
        b.price
            .partial_cmp(&a.price)
            .expect("orders cannot have NaN prices")
    }

    /// Retrieves the effective remaining amount for this order based on user
    /// balances. This is the minimum between the remaining order amount and
    /// the user's sell token balance.
    ///
    /// We can't use `std::cmp::min` here because floats don't implement `Ord`.
    pub fn get_effective_amount(&self, users: &UserMap) -> f64 {
        let balance = users[&self.user].balance_of(self.pair.sell);
        min(self.amount, balance)
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

/// Calculates the minimum of two floats. Note that we cannot use the standard
/// library `std::cmp::min` here since `f64` does not implement `Ord`.
///
/// # Panics
///
/// If any of the two floats are NaN.
fn min(a: f64, b: f64) -> f64 {
    match a
        .partial_cmp(&b)
        .expect("orderbooks cannot have NaN quantities")
    {
        cmp::Ordering::Less => a,
        _ => b,
    }
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

        if let hash_map::Entry::Vacant(entry) = self.balances.entry(element.pair.sell) {
            let balance = u256_to_f64(element.balance);
            if balance >= MIN_AMOUNT {
                entry.insert(balance);
            }
        }
        self.num_orders += 1;

        order_id
    }

    /// Retrieves the user's balance for a token
    fn balance_of(&self, token: TokenId) -> f64 {
        self.balances.get(&token).copied().unwrap_or(0.0)
    }

    /// Deducts an amount from the balance.
    fn deduct_from_balance(&mut self, token: TokenId, amount: f64) {
        if let hash_map::Entry::Occupied(mut entry) = self.balances.entry(token) {
            let balance = entry.get_mut();
            *balance -= amount;
            if *balance < MIN_AMOUNT {
                entry.remove_entry();
            }
        }
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

        let mut orderbook = Orderbook::read(&encoded_orderbook).expect("error reading orderbook");
        assert_eq!(orderbook.num_orders(), 896);

        let overlapping_cycles = orderbook.reduce();
        assert!(!overlapping_cycles.is_empty());
        assert!(orderbook.reduce().is_empty());
    }
}
