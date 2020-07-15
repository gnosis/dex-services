//! Module containing limit price estimation implementation.

use crate::api::TransitiveOrder;
use crate::encoding::TokenPair;
use crate::orderbook::LimitPrice;
use crate::Pricegraph;

impl Pricegraph {
    /// Estimates an exchange rate for the specified token pair and sell volume.
    /// Returns `None` if no combination of orders is able to trade this amount
    /// of the sell token into the buy token. This usually occurs if there is
    /// not enough buy token liquidity in the exchange or if there is no inverse
    /// transitive orders buying the specified sell token for the specified buy
    /// token.
    ///
    /// Note that this price is in exchange format, that is, it is expressed as
    /// the ratio between buy and sell amounts, with implicit fees.
    pub fn estimate_exchange_rate(&self, pair: TokenPair, sell_amount: f64) -> Option<f64> {
        Some(
            self.reduced_orderbook()
                .fill_market_order(pair, sell_amount as _)
                .expect("overlapping orders in reduced orderbook")?
                .value(),
        )
    }

    /// Returns a transitive order with a buy amount calculated such that there
    /// exists overlapping transitive orders to completely fill the specified
    /// `sell_amount`. As such, this is an estimated order that is *likely* to
    /// be matched given the **current** state of the batch.
    pub fn order_for_sell_amount(
        &self,
        pair: TokenPair,
        sell_amount: f64,
    ) -> Option<TransitiveOrder> {
        let price = self.estimate_exchange_rate(pair, sell_amount)?;
        Some(TransitiveOrder {
            buy: sell_amount * price,
            sell: sell_amount,
        })
    }

    /// Returns a transitive order with the largest buy and sell amounts such
    /// that its exchange rate is greater than or equal to the specified limit
    /// exchange rate and there exists overlapping transitive orders to
    /// completely fill the order. Returns `None` if no overlapping transitive
    /// orders exist at the given exchange rate.
    pub fn order_for_limit_exchange_rate(
        &self,
        pair: TokenPair,
        limit_exchange_rate: f64,
    ) -> Option<TransitiveOrder> {
        let (buy, sell) = self
            .reduced_orderbook()
            .fill_order_at_price(pair, LimitPrice::new(limit_exchange_rate)?)
            .expect("overlapping orders in reduced orderbook");
        if buy == 0.0 {
            return None;
        }
        debug_assert!(sell > 0.0, "zero sell amount for non-zero buy amount");

        Some(TransitiveOrder { buy, sell })
    }

    /// Returns a transitive order with the largest sell amount such that there
    /// exists overlapping transitive orders to completely fill the order at the
    /// specified exchange rate. Returns `None` if no overlapping transitive
    /// orders exist at the given exchange rate.
    ///
    /// Note that this method is subtly different to
    /// `Pricegraph::order_for_limit_exchange_rate` in that the exchange rate
    /// for the resulting order is equal to the specified exchange rate.
    pub fn order_at_exchange_rate(
        &self,
        pair: TokenPair,
        exchange_rate: f64,
    ) -> Option<TransitiveOrder> {
        let order = self.order_for_limit_exchange_rate(pair, exchange_rate)?;
        Some(TransitiveOrder {
            buy: order.sell * exchange_rate,
            sell: order.sell,
        })
    }
}
