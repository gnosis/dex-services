//! This module contains fallback token data that should be used by the price
//! estimator when prices are not available.

use crate::models::TokenId;
use crate::price_estimation::price_source::PriceSource;
use anyhow::{anyhow, Context, Error, Result};

use futures::future::{BoxFuture, FutureExt};
use serde::Deserialize;
use std::collections::HashMap;
use std::iter::FromIterator;
use std::num::NonZeroU128;
use std::str::FromStr;

use super::{TokenBaseInfo, TokenInfoFetching};
#[cfg_attr(test, derive(Eq, PartialEq))]
#[derive(Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct TokenInfoOverride {
    pub alias: String,
    pub decimals: u8,
    pub external_price: Option<NonZeroU128>,
}

impl TokenInfoOverride {
    #[cfg(test)]
    pub fn new(alias: &str, decimals: u8, external_price: Option<NonZeroU128>) -> Self {
        Self {
            alias: alias.to_owned(),
            decimals,
            external_price,
        }
    }
}

impl Into<TokenBaseInfo> for TokenInfoOverride {
    fn into(self) -> TokenBaseInfo {
        TokenBaseInfo {
            alias: self.alias,
            decimals: self.decimals,
        }
    }
}

/// Token fallback data containing all fallback information for tokens that
/// should be provided to the solver.
#[cfg_attr(test, derive(Eq, PartialEq))]
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(transparent)]
pub struct TokenData(HashMap<TokenId, TokenInfoOverride>);

impl TokenInfoFetching for TokenData {
    fn get_token_info<'a>(&'a self, id: TokenId) -> BoxFuture<'a, Result<TokenBaseInfo>> {
        let info = self
            .0
            .get(&id)
            .cloned()
            .ok_or_else(|| anyhow!("Token {:?} not found in hardcoded data", id));
        immediate!(Ok(info?.into()))
    }

    fn all_ids<'a>(&'a self) -> BoxFuture<'a, Result<Vec<TokenId>>> {
        let ids = Vec::from_iter(self.0.keys().copied());
        immediate!(Ok(ids))
    }
}

impl PriceSource for TokenData {
    fn get_prices<'a>(
        &'a self,
        tokens: &'a [TokenId],
    ) -> BoxFuture<'a, Result<HashMap<TokenId, NonZeroU128>>> {
        let mut result = HashMap::new();
        for token in tokens {
            if let Some(price) = self.0.get(token).and_then(|info| info.external_price) {
                result.insert(*token, price);
            }
        }
        immediate!(Ok(result))
    }
}

impl From<HashMap<TokenId, TokenInfoOverride>> for TokenData {
    fn from(tokens: HashMap<TokenId, TokenInfoOverride>) -> Self {
        TokenData(tokens)
    }
}

impl Into<HashMap<TokenId, TokenBaseInfo>> for TokenData {
    fn into(self) -> HashMap<TokenId, TokenBaseInfo> {
        self.0
            .into_iter()
            .map(|(id, info)| {
                (
                    id,
                    TokenBaseInfo {
                        alias: info.alias,
                        decimals: info.decimals,
                    },
                )
            })
            .collect()
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
            "externalPrice": 1000000000000000000000000000000
          }
        }"#;

        assert_eq!(
            TokenData::from_str(json).unwrap(),
            TokenData::from(hash_map! {
                TokenId(1) => TokenInfoOverride::new("WETH", 18, Some(nonzero!(200_000_000_000_000_000_000))),
                TokenId(4) => TokenInfoOverride::new("USDC", 6, Some(nonzero!(1_000_000_000_000_000_000_000_000_000_000))),
            })
        );
    }
}
