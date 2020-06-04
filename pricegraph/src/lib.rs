mod encoding;
mod graph;
mod num;
mod orderbook;

#[cfg(test)]
#[path = "../data/mod.rs"]
mod data;

pub use encoding::*;
pub use orderbook::Orderbook;

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
/// - *A*: buying 1_000_000 token 1 selling 2_000_000 token 2
/// - *B*: buying 4_000_000 token 2 selling 1_000_000 token 3
///
/// We can define a transitive order *C* buying 1_000_000 token 1 selling
/// 500_000 token 3 by combining *A* and *B*. Note that the sell amount of token
/// 3 is limited by the token 2 capacity for this transitive order.
///
/// Additionally, a transitive order over a single order is equal to that order.
#[derive(Clone, Debug, PartialEq)]
pub struct TransitiveOrder {
    /// The effective buy amount for this transient order.
    pub buy: f64,
    /// The effective sell amount for this transient order.
    pub sell: f64,
}
