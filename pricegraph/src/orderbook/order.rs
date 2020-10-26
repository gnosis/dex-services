//! Data and logic related to token pair order management.

use super::{map::Map, ExchangeRate, LimitPrice, UserMap};
use crate::encoding::{Element, OrderId, TokenId, TokenPair, UserId};
use primitive_types::U256;
use std::cmp::Reverse;

/// A type for collecting orders and building an order map that garantees that
/// per-pair orders are sorted for optimal access.
#[derive(Debug, Default)]
pub struct OrderCollector(Map<TokenId, Map<TokenId, Vec<Order>>>);

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
            pair_orders.sort_unstable_by_key(|order| Reverse(order.exchange_rate))
        }

        orders
    }
}

/// Type definition for a mapping of orders between buy and sell tokens. Token
/// pair orders are garanteed to be in order, so that the cheapest order is
/// always at the end of the token pair order vector.
#[derive(Clone, Debug)]
pub struct OrderMap(Map<TokenId, Map<TokenId, Vec<Order>>>);

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

    /// Returns an iterator over the orders matching a given sell token.
    pub fn pairs_and_orders_for_sell_token(
        &self,
        sell: TokenId,
    ) -> impl Iterator<Item = (TokenPair, &'_ [Order])> + '_ {
        self.0.get(&sell).into_iter().flat_map(move |o| {
            o.iter()
                .map(move |(&buy, o)| (TokenPair { sell, buy }, o.as_slice()))
        })
    }

    /// Returns the orders for an order pair. Returns `None` if that pair has
    /// no orders.
    fn orders_for_pair(&self, pair: TokenPair) -> Option<&[Order]> {
        Some(self.0.get(&pair.sell)?.get(&pair.buy)?.as_slice())
    }

    /// Returns a mutable reference to orders for an order pair. Returns `None`
    /// if that pair has no orders.
    fn orders_for_pair_mut(&mut self, pair: TokenPair) -> Option<&mut Vec<Order>> {
        self.0.get_mut(&pair.sell)?.get_mut(&pair.buy)
    }

    /// Returns a reference to the cheapest order given an order pair.
    pub fn best_order_for_pair(&self, pair: TokenPair) -> Option<&Order> {
        self.orders_for_pair(pair)?.last()
    }

    /// Returns a mutable reference to the cheapest order given an order pair.
    pub fn best_order_for_pair_mut(&mut self, pair: TokenPair) -> Option<&mut Order> {
        self.orders_for_pair_mut(pair)?.last_mut()
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

/// The remaining amount in an order can be unlimited or fixed.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Amount {
    Unlimited,
    Remaining(u128),
}

/// A single order with a reference to the user.
///
/// Note that we approximate amounts and prices with floating point numbers.
/// While this can lead to rounding errors it greatly simplifies the graph
/// computations and still leads to acceptable estimates.
#[derive(Clone, Debug)]
pub struct Order {
    /// The user owning the order.
    pub user: UserId,
    /// The index of an order per user.
    pub id: OrderId,
    /// The token pair.
    pub pair: TokenPair,
    /// The maximum capacity for this order, this is equivalent to the order's
    /// remaining sell amount. Note that orders are also limited by their user's
    /// balance.
    pub amount: Amount,
    /// The effective exchange rate for this order.
    pub exchange_rate: ExchangeRate,
}

impl Order {
    /// Creates a new order from an ID and an orderbook element.
    pub fn new(element: &Element) -> Option<Self> {
        let amount = if is_unbounded(&element) {
            Amount::Unlimited
        } else {
            Amount::Remaining(element.remaining_sell_amount)
        };
        let exchange_rate = LimitPrice::from_fraction(&element.price)?.exchange_rate();

        Some(Order {
            user: element.user,
            id: element.id,
            pair: element.pair,
            amount,
            exchange_rate,
        })
    }

    /// Retrieves the effective remaining amount for this order based on user
    /// balances. This is the minimum between the remaining order amount and
    /// the user's sell token balance.
    pub fn get_effective_amount(&self, users: &UserMap) -> U256 {
        let balance = users[&self.user].balance_of(self.pair.sell);
        match self.amount {
            Amount::Unlimited => balance,
            Amount::Remaining(amount) => balance.min(amount.into()),
        }
    }
}

/// Returns `true` if the order is unbounded, that is it has an unlimited sell
/// amount.
fn is_unbounded(element: &Element) -> bool {
    const UNBOUNDED_AMOUNT: u128 = u128::max_value();
    element.price.numerator == UNBOUNDED_AMOUNT || element.price.denominator == UNBOUNDED_AMOUNT
}
