//! This crate provides a thin WASM-compatible wrapper around the `pricegraph`
//! crate and can be used for estimating prices for a given orderbook.

use pricegraph::{Orderbook, TokenId, TokenPair};
use wasm_bindgen::prelude::*;

/// A graph representation of a complete orderbook.
#[wasm_bindgen]
pub struct PriceEstimator {
    orderbook: Orderbook,
}

#[wasm_bindgen]
impl PriceEstimator {
    /// Creates a `PriceEstimator` instance by reading an orderbook from encoded
    /// bytes. Returns an error if the encoded orders are invalid.
    #[wasm_bindgen(constructor)]
    pub fn new(bytes: &[u8]) -> Result<PriceEstimator, JsValue> {
        console_error_panic_hook::set_once();

        let mut orderbook = Orderbook::read(bytes).map_err(|err| err.to_string())?;
        orderbook.reduce_overlapping_orders();

        Ok(PriceEstimator { orderbook })
    }

    /// Estimates price for the specified trade. Returns `undefined` if the
    /// volume cannot be fully filled.
    #[wasm_bindgen(js_name = "estimatePrice")]
    pub fn estimate_price(&self, buy: TokenId, sell: TokenId, volume: f64) -> Option<f64> {
        // NOTE: Make sure to use a copy of the orderbook so that successive
        // calls to `estimate_price` do not affect eachother.
        self.orderbook
            .clone()
            .fill_market_order(TokenPair { buy, sell }, volume)
    }
}
