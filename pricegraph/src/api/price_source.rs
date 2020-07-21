//! This module implements price source methods for the `Pricegraph` API so that
//! it can be used for OWL price estimates to the solver.

use crate::encoding::TokenPair;
use crate::Pricegraph;

impl Pricegraph {
    /// Estimates the fee token price in WEI for the specified token. Returns
    /// `None` if the token is not connected to the fee token.
    ///
    /// The fee token is defined as the token with ID 0.
    pub fn token_price(&self, token: TokenId) -> Option<f64> {
        todo!()
    }
}
