//! Module implements common data types for tokens on the exchange.

use anyhow::{Context, Error, Result};
use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::borrow::Cow;
use std::collections::HashMap;
use std::str::FromStr;

/// A token ID wrapper type that implements JSON serialization in the solver
/// format.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialOrd, PartialEq)]
pub struct TokenId(pub u16);

impl TokenId {
    /// Returns the token ID of the fee token.
    pub fn reference() -> Self {
        TokenId(0)
    }
}

impl<'de> Deserialize<'de> for TokenId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let key = Cow::<str>::deserialize(deserializer)?;
        if !key.starts_with('T') || key.len() != 5 {
            return Err(D::Error::custom("Token ID must be of the form 'Txxxx'"));
        }

        let id = key[1..].parse::<u16>().map_err(D::Error::custom)?;
        Ok(TokenId(id))
    }
}

impl Serialize for TokenId {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        format!("T{:04}", self.0).serialize(serializer)
    }
}

impl Into<u16> for TokenId {
    fn into(self) -> u16 {
        self.0
    }
}

impl From<u16> for TokenId {
    fn from(id: u16) -> Self {
        TokenId(id)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct TokenInfo {
    pub alias: String,
    pub decimals: u8,
    pub external_price: u128,
}

impl TokenInfo {
    /// Utility method for creating a token for unit tests.
    #[cfg(test)]
    pub fn test(alias: &str, decimals: u8, external_price: u128) -> Self {
        TokenInfo {
            alias: alias.into(),
            decimals,
            external_price,
        }
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct TokenData(HashMap<TokenId, TokenInfo>);

impl TokenData {
    /// Retrieves some token information from a token ID.
    pub fn info(&self, id: impl Into<TokenId>) -> Option<&TokenInfo> {
        self.0.get(&id.into())
    }

    /// Utility method for creating a token for unit tests.
    #[cfg(test)]
    pub fn test(tokens: HashMap<TokenId, TokenInfo>) -> Self {
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
