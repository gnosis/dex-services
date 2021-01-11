//! This module contains fallback token data that should be used by the price
//! estimator when prices are not available.

use crate::{models::TokenId, price_estimation::price_source::PriceSource};
use anyhow::{anyhow, Context, Error, Result};
use ethcontract::Address;
use serde::Deserialize;
use std::{collections::HashMap, num::NonZeroU128, str::FromStr};

use super::{TokenBaseInfo, TokenInfoFetching};
#[cfg_attr(test, derive(Eq, PartialEq))]
#[derive(Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct TokenInfoOverride {
    pub address: Address,
    pub alias: String,
    pub decimals: u8,
    pub external_price: Option<NonZeroU128>,
}

impl TokenInfoOverride {
    #[cfg(test)]
    pub fn new(
        address: Address,
        alias: &str,
        decimals: u8,
        external_price: Option<NonZeroU128>,
    ) -> Self {
        Self {
            address,
            alias: alias.to_owned(),
            decimals,
            external_price,
        }
    }
}

impl Into<TokenBaseInfo> for TokenInfoOverride {
    fn into(self) -> TokenBaseInfo {
        TokenBaseInfo {
            address: self.address,
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

#[async_trait::async_trait]
impl TokenInfoFetching for TokenData {
    async fn get_token_info(&self, id: TokenId) -> Result<TokenBaseInfo> {
        self.0
            .get(&id)
            .cloned()
            .ok_or_else(|| anyhow!("Token {:?} not found in hardcoded data", id))
            .map(Into::into)
    }

    async fn all_ids(&self) -> Result<Vec<TokenId>> {
        Ok(self.0.keys().copied().collect())
    }
}

#[async_trait::async_trait]
impl PriceSource for TokenData {
    async fn get_prices(&self, tokens: &[TokenId]) -> Result<HashMap<TokenId, NonZeroU128>> {
        let mut result = HashMap::new();
        for token in tokens {
            if let Some(price) = self.0.get(token).and_then(|info| info.external_price) {
                result.insert(*token, price);
            }
        }
        Ok(result)
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
                        address: Address::from_low_u64_be(0),
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
            "address": "0x000000000000000000000000000000000000000a",
            "alias": "WETH",
            "decimals": 18,
            "externalPrice": 200000000000000000000
          },
          "T0004": {
            "address": "0x000000000000000000000000000000000000000B",
            "alias": "USDC",
            "decimals": 6,
            "externalPrice": 1000000000000000000000000000000
          }
        }"#;

        assert_eq!(
            TokenData::from_str(json).unwrap(),
            TokenData::from(hash_map! {
                TokenId(1) => TokenInfoOverride::new(Address::from_low_u64_be(10), "WETH", 18, Some(nonzero!(200_000_000_000_000_000_000))),
                TokenId(4) => TokenInfoOverride::new(Address::from_low_u64_be(11), "USDC", 6, Some(nonzero!(1_000_000_000_000_000_000_000_000_000_000))),
            })
        );
    }
}
