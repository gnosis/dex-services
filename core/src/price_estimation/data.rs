//! This module contains fallback token data that should be used by the price
//! estimator when prices are not available.

use crate::models::{TokenId, TokenInfo};
use anyhow::{Context, Error, Result};
use lazy_static::lazy_static;
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

    /// Retrieves the token symbol for this token.
    ///
    /// Note that the token info alias is first checked if it is part of a
    /// symbol override map, and if it is, then that value is used instead. This
    /// allows ERC20 tokens like WETH to be treated as ETH, since exchanges
    /// generally only track prices for the latter.
    pub fn symbol(&self) -> &str {
        lazy_static! {
            static ref SYMBOL_OVERRIDES: HashMap<String, String> = hash_map! {
                "WETH" => "ETH".to_owned(),
            };
        }

        SYMBOL_OVERRIDES.get(&self.alias).unwrap_or(&self.alias)
    }

    /// Converts the prices from USD into the unit expected by the contract.
    /// This price is relative to the OWL token which is considered pegged at
    /// exactly 1 USD with 18 decimals.
    pub fn get_owl_price(&self, usd_price: f64) -> u128 {
        let pow = 36 - (self.decimals as i32);
        (usd_price * 10f64.powi(pow)) as _
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

    /// Returns true if the token data is empty and contains no token infos.
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Returns a vector with all the tokens that should be priced in the token
    /// data map.
    pub fn all_tokens_to_estimate_price(&self) -> Vec<TokenId> {
        self.0
            .iter()
            .filter(|&(_, info)| info.should_estimate_price)
            .map(|(&id, _)| id)
            .collect()
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
