//! Data and logic related to token pair order management.

#![allow(dead_code)]

use super::UserMap;
use crate::encoding::{Element, Price, TokenId, TokenPair, UserId};
use crate::num;
use std::cmp;
use std::collections::HashMap;
use std::f64;

/// A type for collecting orders and building an order map that garantees that
/// per-pair orders are sorted for optimal access.
#[derive(Debug, Default)]
pub struct OrderCollector(HashMap<TokenId, HashMap<TokenId, Vec<Order>>>);

impl OrderCollector {
    /// Adds a new order to the order map.
    pub fn insert_order(&mut self, order: Order) {
        self.0
            .entry(order.pair.sell)
            .or_default()
            .entry(order.pair.buy)
            .or_default()
            .push(order);
    }

    /// Builds an order map from the inserted orders, ensuring that the orders
    /// are sorted the orders per token pair in descending order, this makes it
    /// so the last order is the cheapest one and can be determined given a
    /// token pair in `O(1)`, and it can simply be `pop`-ed once its amount is
    /// used up and removed from the graph in `O(1)` as well.
    pub fn collect(self) -> OrderMap {
        let mut orders = OrderMap(self.0);
        for (_, pair_orders) in orders.all_pairs_mut() {
            pair_orders.sort_unstable_by(Order::cmp_descending_prices);
        }

        orders
    }
}

/// Type definition for a mapping of orders between buy and sell tokens. Token
/// pair orders are garanteed to be in order, so that the cheapest order is
/// always at the end of the token pair order vector.
#[derive(Debug)]
pub struct OrderMap(HashMap<TokenId, HashMap<TokenId, Vec<Order>>>);

impl OrderMap {
    /// Returns an iterator over the collection of orders for each token pair.
    pub fn all_pairs(&self) -> impl Iterator<Item = (TokenPair, &'_ [Order])> + '_ {
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
    fn pair_orders(&self, pair: TokenPair) -> Option<&[Order]> {
        Some(self.0.get(&pair.sell)?.get(&pair.buy)?.as_slice())
    }

    /// Returns a mutable reference to orders for an order pair. Returns `None`
    /// if that pair has no orders.
    fn pair_orders_mut(&mut self, pair: TokenPair) -> Option<&mut Vec<Order>> {
        self.0.get_mut(&pair.sell)?.get_mut(&pair.buy)
    }

    /// Returns a reference to the cheapest order given an order pair.
    pub fn pair_order(&self, pair: TokenPair) -> Option<&Order> {
        self.pair_orders(pair)?.last()
    }

    /// Returns a mutable reference to the cheapest order given an order pair.
    pub fn pair_order_mut(&mut self, pair: TokenPair) -> Option<&mut Order> {
        self.pair_orders_mut(pair)?.last_mut()
    }

    /// Removes the current cheapest order pair from the mapping.
    pub fn remove_pair_order(&mut self, pair: TokenPair) -> Option<Order> {
        let sell_orders = self.0.get_mut(&pair.sell)?;
        let pair_orders = sell_orders.get_mut(&pair.buy)?;

        let removed = pair_orders.pop();

        if pair_orders.is_empty() {
            sell_orders.remove(&pair.buy)?;
        }
        if sell_orders.is_empty() {
            self.0.remove(&pair.sell);
        }

        removed
    }
}

/// A single order with a reference to the user.
///
/// Note that we approximate amounts and prices with floating point numbers.
/// While this can lead to rounding errors it greatly simplifies the graph
/// computations and still leads to acceptable estimates.
#[derive(Debug, PartialEq)]
pub struct Order {
    /// The user owning the order.
    pub user: UserId,
    /// The index of an order per user.
    pub index: usize,
    /// The token pair.
    pub pair: TokenPair,
    /// The maximum capacity for this order, this is equivalent to the order's
    /// remaining sell amount. Note that orders are also limited by their user's
    /// balance.
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
    /// Creates a new order from an ID and an orderbook element.
    pub fn new(element: Element, index: usize) -> Self {
        let amount = if is_unbounded(&element) {
            f64::INFINITY
        } else {
            element.remaining_sell_amount as _
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

    /// Retrieves the effective remaining amount for this order based on user
    /// balances. This is the minimum between the remaining order amount and
    /// the user's sell token balance.
    pub fn get_effective_amount(&self, users: &UserMap) -> f64 {
        let balance = users[&self.user].balance_of(self.pair.sell);
        num::min(self.amount, balance)
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
    FEE_FACTOR * (price.numerator as f64) / (price.denominator as f64)
}
