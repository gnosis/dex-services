//! Module containing data for representing flow through the orderbook graph.

use crate::{FEE_FACTOR, TransitiveOrder};
use petgraph::graph::NodeIndex;

/// A reprensentation of a flow of tokens through the orderbook graph.
pub struct Flow {
    /// The path through the graph for this flow.
    pub path: Vec<NodeIndex>,
    /// The optimal effective exchange rate for trading the start token for the
    /// end token along the path in the graph.
    pub exchange_rate: f64,
    /// The total capacity the path can accomodate.
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
        let sell = self.capacity / self.exchange_rate;

        TransitiveOrder { buy, sell }
    }
}
