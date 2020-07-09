//! Module containing data for representing flow through the orderbook graph.

use super::ExchangeRate;
use crate::{TransitiveOrder, FEE_FACTOR};

/// A reprensentation of a flow of tokens through the orderbook graph.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Flow {
    /// The effective exchange rate for trading along a path in the graph.
    pub exchange_rate: ExchangeRate,
    /// The total capacity the path can accomodate expressed in the starting
    /// token, which is the buy token for the transitive order along a path.
    pub capacity: f64,
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
        let sell = self.capacity / *self.exchange_rate;

        TransitiveOrder { buy, sell }
    }
}
