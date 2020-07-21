//! Module implements common data types for tokens on the exchange.

use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::{
    borrow::Cow,
    fmt::{self, Display, Formatter},
};

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

impl Display for TokenId {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        Display::fmt(&self.0, f)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct TokenInfo {
    pub alias: Option<String>,
    pub decimals: Option<u8>,
    pub external_price: u128,
}

impl TokenInfo {
    /// Create new token information from its parameters.
    #[cfg(test)]
    pub fn new(alias: impl Into<String>, decimals: u8, external_price: u128) -> Self {
        TokenInfo {
            alias: Some(alias.into()),
            decimals: Some(decimals),
            external_price,
        }
    }
}
