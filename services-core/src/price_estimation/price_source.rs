use crate::models::{TokenId, TokenInfo};
use anyhow::Result;
use std::collections::HashMap;
use std::num::NonZeroU128;

/// A token representation.
#[cfg_attr(test, derive(Eq, PartialEq))]
#[derive(Clone, Debug)]
pub struct Token {
    /// The ID of the token.
    pub id: TokenId,
    /// The token info for this token including, token symbol and number of
    /// decimals.
    pub info: TokenInfo,
}

/// An abstraction around a type that retrieves price estimate from a source
/// such as an exchange.
#[cfg_attr(test, mockall::automock)]
#[async_trait::async_trait]
pub trait PriceSource {
    /// Retrieve current prices relative to the OWL token for the specified
    /// tokens (price denominated in OWL). The OWL token is pegged at 1 USD
    /// with 18 decimals. Returns a sparse price array as being unable to
    /// find a price is not considered an error.
    async fn get_prices(&self, tokens: &[TokenId]) -> Result<HashMap<TokenId, NonZeroU128>>;
}

/// A no-op price source that always succeeds and finds no prices.
pub struct NoopPriceSource;

#[async_trait::async_trait]
impl PriceSource for NoopPriceSource {
    async fn get_prices(&self, _: &[TokenId]) -> Result<HashMap<TokenId, NonZeroU128>> {
        Ok(HashMap::new())
    }
}
