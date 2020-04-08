//! Data and logic related to token pair order management.

use crate::encoding::{Element, Price, TokenId, TokenPair, UserId};
use std::collections::HashMap;
use std::f64;
use std::cmp;

/// Type definition for a mapping of orders between buy and sell tokens.
#[derive(Debug, Default)]
pub struct OrderMap(HashMap<TokenId, HashMap<TokenId, Vec<Order>>>);

impl OrderMap {
    /// Adds a new order to the order map.
    pub fn insert_order(&mut self, order: Order) {
        self.0
            .entry(order.pair.sell)
            .or_default()
            .entry(order.pair.buy)
            .or_default()
            .push(order);
    }

    /// Returns an iterator over the collection of orders for each token pair.
    pub fn all_pairs(&self) -> impl Iterator<Item = (TokenPair, &'_ [Order])> + '_ {
        self.0.iter().flat_map(|(&sell, o)| {
            o.iter()
                .map(move |(&buy, o)| (TokenPair { sell, buy }, o.as_slice()))
        })
    }

    /// Returns an iterator over the collection of orders for each token pair.
    pub fn all_pairs_mut(&mut self) -> impl Iterator<Item = (TokenPair, &'_ mut Vec<Order>)> + '_ {
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
pub struct Order {
    /// The user owning the order.
    pub user: UserId,
    /// The index of an order per user.
    pub index: usize,
    /// The token pair.
    pub pair: TokenPair,
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
    /// The weight of the order in the graph. This is the base-2 logarithm of
    /// the price including fees. This enables transitive prices to be computed
    /// using addition.
    pub weight: f64,
}

impl Order {
    /// Creates a new order from an ID and an orderbook element.
    pub fn new(element: Element, index: usize) -> Self {
        let amount = if is_unbounded(&element) {
            f64::INFINITY
        } else {
            element.price.denominator as _
        };
        let price = as_effective_sell_price(&element.price);
        let weight = price.log2();

        Order {
            user: element.user,
            index,
            pair: element.pair,
            amount,
            price,
            weight,
        }
    }

    /// Compare two orders by descending price order.
    ///
    /// This method is used for sorting orders, instead of just sorting by key
    /// on `price` field because `f64`s don't implement `Ord` and as such cannot
    /// be used for sorting. This be because there is no real ordering for
    /// `NaN`s and `NaN < 0 == false` and `NaN >= 0 == false` (cf. IEEE 754-2008
    /// section 5.11), which can cause serious problems with sorting.
    pub fn cmp_decending_prices(a: &Order, b: &Order) -> cmp::Ordering {
        b.price
            .partial_cmp(&a.price)
            .expect("orders cannot have NaN prices")
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
