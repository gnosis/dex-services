//! Module containing data for representing flow through the orderbook graph.

use super::ExchangeRate;
use crate::num;
use crate::{TransitiveOrder, FEE_FACTOR};

/// A reprensentation of a flow of tokens through the orderbook graph.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Flow {
    /// The effective exchange rate for trading along a path in the graph.
    pub exchange_rate: ExchangeRate,
    /// The total capacity the path can accomodate expressed in the starting
    /// token, which is the buy token for the transitive order along a path.
    pub capacity: f64,
    /// The minimum traded amount along a path.
    pub min_trade: f64,
}

impl Flow {
    /// Convert this flow into a transitive order.
    pub fn as_transitive_order(&self) -> TransitiveOrder {
        // NOTE: The flow's capacity and exchange rate needs to be converted to
        // a buy and sell amount. We have:
        // - `price = FEE_FACTOR * buy_amount / sell_amount`
        // - `capacity = sell_amount * price`
        // Solving for `buy_amount` and `sell_amount`, we get:
        let buy = self.capacity / FEE_FACTOR;
        let sell = self.capacity / self.exchange_rate.value();

        TransitiveOrder { buy, sell }
    }

    /// Returns true if this flow is a dust trade.
    pub fn is_dust_trade(&self) -> bool {
        num::is_dust_amount(self.min_trade as u128)
    }
}

/// A representation of flow on two halves of a ring trade through the orderbook
/// graph for a market.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Ring {
    /// The "ask" flow, starting from a market's quote token and ending at the
    /// base token.
    pub ask: Flow,
    /// The "bid" flow, starting from a market's base token and ending at the
    /// quote token.
    pub bid: Flow,
}
