//! Module containing helpers for analyzing exchange history.

use core::{contracts::stablex_contract::batch_exchange::event_data::Trade, history::Settlement};
use pricegraph::{Element, TokenId, UserId};
use std::collections::HashMap;

/// An ID for uniquely identifying orders.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
pub struct OrderId(pub UserId, pub pricegraph::OrderId);

impl OrderId {
    /// Extracts an order ID from an auction element.
    pub fn from_element(element: &Element) -> Self {
        OrderId(element.user, element.id)
    }

    /// Extracts an order ID from a trade event.
    pub fn from_trade(trade: &Trade) -> Self {
        OrderId(trade.owner, trade.order_id)
    }
}

impl PartialEq<Element> for OrderId {
    fn eq(&self, rhs: &Element) -> bool {
        self == &OrderId::from_element(rhs)
    }
}

/// A struct to represent traded amounts.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct TradeAmounts {
    pub executed_buy_amount: u128,
    pub executed_sell_amount: u128,
}

impl TradeAmounts {
    /// Extracts traded amounts from a trade event.
    pub fn from_trade(trade: &Trade) -> Self {
        TradeAmounts {
            executed_buy_amount: trade.executed_buy_amount,
            executed_sell_amount: trade.executed_sell_amount,
        }
    }
    /// Returns an exchange rate for the exectued trade.
    pub fn exchange_rate(&self) -> f64 {
        self.executed_buy_amount as f64 / self.executed_sell_amount as f64
    }
}

/// A simplified batch settlement representation.
#[derive(Clone, Debug, Default)]
pub struct SettlementSummary {
    pub prices: HashMap<TokenId, u128>,
    pub trades: HashMap<OrderId, TradeAmounts>,
}

impl SettlementSummary {
    /// Returns a settlement summary computed from a settlement containing trade
    /// and solution events.
    ///
    /// # Panics
    ///
    /// Panics if the solution data is invalid and pr
    pub fn summarize(settlement: &Settlement) -> Self {
        let token_ids = &settlement.solution.token_ids_for_price;
        let prices = &settlement.solution.prices;
        debug_assert_eq!(
            token_ids.len(),
            prices.len(),
            "invalid solution price mapping",
        );

        let prices = token_ids
            .iter()
            .copied()
            .zip(prices.iter().copied())
            .collect();
        let trades = settlement
            .trades
            .iter()
            .map(|trade| (OrderId::from_trade(trade), TradeAmounts::from_trade(trade)))
            .collect();

        SettlementSummary { prices, trades }
    }
}
