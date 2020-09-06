//! Module containing the high-level `Pricegraph` API operation implementations.

mod price_estimation;
mod price_source;
mod transitive_orderbook;

pub use self::transitive_orderbook::TransitiveOrderbook;
use crate::encoding::{TokenId, TokenPair};
use crate::FEE_FACTOR;

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
    /// The effective buy amount for this transitive order.
    pub buy: f64,
    /// The effective sell amount for this transitive order.
    pub sell: f64,
}

impl TransitiveOrder {
    /// Retrieves the exchange rate for this order.
    pub fn exchange_rate(&self) -> f64 {
        self.buy / self.sell
    }

    /// Retrieves the effective exchange rate for this order after fees are
    /// condidered.
    ///
    /// Note that `effective_exchange_rate > exchange_rate`.
    pub fn effective_exchange_rate(&self) -> f64 {
        self.exchange_rate() * FEE_FACTOR
    }

    /// Retrieves the minimum exchange rate such that it overlaps with the
    /// transitive order, accounting for fees on both sides of the trade.
    pub fn overlapping_exchange_rate(&self) -> f64 {
        1.0 / (self.exchange_rate() * FEE_FACTOR.powi(2))
    }
}

/// A struct representing a market.
///
/// This is used for computing transitive orderbooks.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
#[cfg_attr(feature = "fuzz", derive(arbitrary::Arbitrary))]
pub struct Market {
    /// The base or transaction token.
    pub base: TokenId,
    /// The quote or counter token to be used as the reference token in the
    /// market. Prices in a market are always expressed in the quote token.
    pub quote: TokenId,
}

impl Market {
    /// Returns the token pair for ask orders.
    pub fn ask_pair(self) -> TokenPair {
        TokenPair {
            buy: self.quote,
            sell: self.base,
        }
    }

    /// Returns the token pair for bid orders.
    pub fn bid_pair(self) -> TokenPair {
        TokenPair {
            buy: self.base,
            sell: self.quote,
        }
    }

    /// Returns the inverse market.
    pub fn inverse(self) -> Market {
        Market {
            base: self.quote,
            quote: self.base,
        }
    }
}
