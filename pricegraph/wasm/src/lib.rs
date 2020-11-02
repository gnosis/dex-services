//! This crate provides a thin WASM-compatible wrapper around the `pricegraph`
//! crate and can be used for estimating prices for a given orderbook.

use pricegraph::{Pricegraph, TokenId, TokenPair};
use wasm_bindgen::prelude::*;

/// A graph representation of a complete orderbook.
#[wasm_bindgen]
pub struct PriceEstimator {
    pricegraph: Pricegraph,
}

#[wasm_bindgen]
impl PriceEstimator {
    /// Creates a `PriceEstimator` instance by reading an orderbook from encoded
    /// bytes. Returns an error if the encoded orders are invalid.
    ///
    /// The orders are expected to be encoded indexed orders, in the same format
    /// as `BatchExchangeViewer::getFilteredOrdersPaginated`. Specifically each
    /// order has a `114` byte stride with the following values (appearing in
    /// encoding order, all values are little endian encoded).
    /// - `20` bytes: owner's address
    /// - `32` bytes: owners's sell token balance
    /// - `2` bytes: buy token ID
    /// - `2` bytes: sell token ID
    /// - `4` bytes: valid from batch ID
    /// - `4` bytes: valid until batch ID
    /// - `16` bytes: price numerator
    /// - `16` bytes: price denominator
    /// - `16` bytes: remaining order sell amount
    /// - `2` bytes: order ID
    #[wasm_bindgen(constructor)]
    pub fn new(bytes: &[u8]) -> Result<PriceEstimator, JsValue> {
        console_error_panic_hook::set_once();
        let pricegraph = Pricegraph::read(bytes).map_err(|err| err.to_string())?;

        Ok(PriceEstimator { pricegraph })
    }

    /// Estimates price for the specified trade. Returns `undefined` if the
    /// volume cannot be fully filled.
    #[wasm_bindgen(js_name = "estimatePrice")]
    pub fn estimate_price(&self, buy: TokenId, sell: TokenId, volume: f64) -> Option<f64> {
        self.pricegraph
            .estimate_limit_price(TokenPair { buy, sell }.into_unbounded_range(), volume)
    }
}
