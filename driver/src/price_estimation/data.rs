//! This module contains fallback token data that should be used by the price
//! estimator when prices are not available.

use crate::models::{TokenId, TokenInfo};
use anyhow::{Context, Error, Result};
use serde::Deserialize;
use std::collections::HashMap;
use std::str::FromStr;

/// Base token info to use for providing token information to the solver. This
/// differs slightly from the `TokenInfo` type in that is allows some extra
/// parameters that are used by the `price_estimation` module but do not get
/// passed to the solver.
#[cfg_attr(test, derive(Eq, PartialEq))]
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TokenBaseInfo {
    // NOTE: We have to, unfortunately duplicate fields and cannot use
    //   `#[serde(flatten)]` as it does not interact correctly with `u128`s:
    //   https://github.com/serde-rs/json/issues/625
    pub alias: String,
    pub decimals: u8,
    pub external_price: u128,
    #[serde(default)]
    pub should_estimate_price: bool,
}

impl TokenBaseInfo {
    /// Create new token information from its parameters.
    #[cfg(test)]
    pub fn new(
        alias: impl Into<String>,
        decimals: u8,
        external_price: u128,
        should_estimate_price: bool,
    ) -> Self {
        TokenBaseInfo {
            alias: alias.into(),
            decimals,
            external_price,
            should_estimate_price,
        }
    }
}

impl Into<TokenInfo> for TokenBaseInfo {
    fn into(self) -> TokenInfo {
        TokenInfo {
            alias: self.alias,
            decimals: self.decimals,
            external_price: self.external_price,
        }
    }
}

/// Token fallback data containing all fallback information for tokens that
/// should be provided to the solver.
#[cfg_attr(test, derive(Eq, PartialEq))]
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(transparent)]
pub struct TokenData(HashMap<TokenId, TokenBaseInfo>);

impl TokenData {
    /// Retrieves some token information from a token ID.
    pub fn info(&self, id: impl Into<TokenId>) -> Option<&TokenBaseInfo> {
        self.0.get(&id.into())
    }
}

impl From<HashMap<TokenId, TokenBaseInfo>> for TokenData {
    fn from(tokens: HashMap<TokenId, TokenBaseInfo>) -> Self {
        TokenData(tokens)
    }
}

impl FromStr for TokenData {
    type Err = Error;

    fn from_str(token_data: &str) -> Result<Self> {
        Ok(serde_json::from_str(token_data)
            .context("failed to parse token data from JSON string")?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn token_fallback_data_from_str() {
        let json = r#"{
          "T0001": {
            "alias": "WETH",
            "decimals": 18,
            "externalPrice": 200000000000000000000
          },
          "T0004": {
            "alias": "USDC",
            "decimals": 6,
            "externalPrice": 1000000000000000000000000000000,
            "shouldEstimatePrice": true
          }
        }"#;

        assert_eq!(
            TokenData::from_str(json).unwrap(),
            TokenData::from(hash_map! {
                TokenId(1) => TokenBaseInfo::new("WETH", 18, 200_000_000_000_000_000_000, false),
                TokenId(4) => TokenBaseInfo::new("USDC", 6, 1_000_000_000_000_000_000_000_000_000_000, true),
            })
        );
    }
}
