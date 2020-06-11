mod encoding;
mod graph;
mod num;
mod orderbook;

#[cfg(test)]
#[path = "../data/mod.rs"]
mod data;

pub use encoding::*;
pub use orderbook::Orderbook;

/// The fee factor that is applied to each order's buy price.
pub const FEE_FACTOR: f64 = 1.0 / 0.999;

/// A struct representing a transitive orderbook for a base and quote token.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TransitiveOrderbook {
    /// Transitive "ask" orders, i.e. transitive orders buying the base token
    /// and selling the quote token.
    pub asks: Vec<TransitiveOrder>,
    /// Transitive "bid" orders, i.e. transitive orders buying the quote token
    /// and selling the base token.
    pub bids: Vec<TransitiveOrder>,
}

/// A struct representing a transitive order for trading between two tokens.
///
/// A transitive order is defined as the transitive combination of multiple
/// orders into a single equivalent order. For example consider the following
/// two orders:
/// - *A*: buying 1.0 token 1 selling 2.0 token 2
/// - *B*: buying 4.0 token 2 selling 1.0 token 3
///
/// We can define a transitive order *C* buying 1.0 token 1 selling 0.5 token 3
/// by combining *A* and *B*. Note that the sell amount of token 3 is limited by
/// the token 2 capacity for this transitive order.
///
/// Additionally, a transitive order over a single order is equal to that order.
#[derive(Clone, Debug, PartialEq)]
pub struct TransitiveOrder {
    price: f64,
    capacity: f64,
}

impl TransitiveOrder {
    /// ?
    pub fn price(&self) -> f64 {
        self.price
    }

    /// ?
    pub fn capacity(&self) -> f64 {
        self.capacity
    }

    // NOTE: We have the capacity and price for this transitive order which
    // needs to be converted to a buy and sell amount. We have:
    // - `price = FEE_FACTOR * buy_amount / sell_amount`
    // - `capacity = sell_amount * price`
    // Solving for `buy_amount` and `sell_amount`, we get:

    /// The effective buy amount for this transient order.
    pub fn buy(&self) -> f64 {
        self.capacity / FEE_FACTOR
    }

    /// The effective sell amount for this transient order.
    pub fn sell(&self) -> f64 {
        self.capacity / self.price
    }
}
