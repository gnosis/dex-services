use super::Token;
use crate::models::TokenId;
use anyhow::Result;
use std::collections::HashMap;

/// An abstraction around a type that retrieves price estimate from a source
/// such as an exchange.
#[cfg_attr(test, mockall::automock)]
pub trait PriceSource {
    /// Retrieve current prices relative to the OWL token for the specified
    /// tokens. The OWL token is peged at 1 USD with 18 decimals. Returns a
    /// sparce price array as being unable to find a price is not considered an
    /// error.
    fn get_prices(&self, tokens: &[Token]) -> Result<HashMap<TokenId, u128>>;
}
