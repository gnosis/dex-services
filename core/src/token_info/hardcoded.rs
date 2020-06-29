//! This module contains fallback token data that should be used by the price
//! estimator when prices are not available.

use crate::models::TokenId;
use anyhow::{anyhow, Context, Error, Result};

use serde::Deserialize;
use std::collections::HashMap;
use std::iter::FromIterator;
use std::str::FromStr;

use super::{TokenBaseInfo, TokenInfoFetching};

/// Token fallback data containing all fallback information for tokens that
/// should be provided to the solver.
#[cfg_attr(test, derive(Eq, PartialEq))]
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(transparent)]
pub struct TokenData(HashMap<TokenId, TokenBaseInfo>);

impl TokenInfoFetching for TokenData {
    fn get_token_info(&self, id: TokenId) -> Result<TokenBaseInfo> {
        self.0
            .get(&id.into())
            .cloned()
            .ok_or(anyhow!("Token {:?} not found in hardcoded data", id))
    }

    fn all_ids(&self) -> Result<Vec<TokenId>> {
        Ok(Vec::from_iter(self.0.keys().copied()))
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
                TokenId(1) => TokenBaseInfo::new("WETH", 18, 200_000_000_000_000_000_000),
                TokenId(4) => TokenBaseInfo::new("USDC", 6, 1_000_000_000_000_000_000_000_000_000_000),
            })
        );
    }

    #[test]
    fn token_get_price() {
        for (token, usd_price, expected) in &[
            (
                TokenBaseInfo::new("USDC", 6, 0),
                0.99,
                0.99 * 10f64.powi(30),
            ),
            (
                TokenBaseInfo::new("DAI", 18, 0),
                1.01,
                1.01 * 10f64.powi(18),
            ),
            (TokenBaseInfo::new("FAKE", 32, 0), 1.0, 10f64.powi(4)),
            (
                TokenBaseInfo::new("SCAM", 42, 0),
                10f64.powi(10),
                10f64.powi(4),
            ),
        ] {
            let owl_price = token.get_owl_price(*usd_price);
            assert_eq!(owl_price, *expected as u128);
        }
    }

    #[test]
    fn token_get_price_without_rounding_error() {
        assert_eq!(
            TokenBaseInfo::new("OWL", 18, 0).get_owl_price(1.0),
            1_000_000_000_000_000_000,
        );
    }

    #[test]
    fn weth_token_symbol_is_eth() {
        assert_eq!(TokenBaseInfo::new("WETH", 18, 0).symbol(), "ETH");
    }
}
