use anyhow::Result;
use futures::future::BoxFuture;
use lazy_static::lazy_static;
#[cfg(test)]
use mockall::automock;
use serde::Deserialize;
use std::collections::HashMap;

use crate::models::{TokenId, TokenInfo};
pub mod hardcoded;

#[cfg_attr(test, automock)]
pub trait TokenInfoFetching: Send + Sync {
    /// Retrieves some token information from a token ID.
    fn get_token_info<'a>(&'a self, id: TokenId) -> BoxFuture<'a, Result<TokenBaseInfo>>;

    /// Returns a vector with all the token IDs available
    fn all_ids<'a>(&'a self) -> BoxFuture<'a, Result<Vec<TokenId>>>;
}

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
}

impl TokenBaseInfo {
    /// Create new token information from its parameters.
    #[cfg(test)]
    pub fn new(alias: impl Into<String>, decimals: u8, external_price: u128) -> Self {
        TokenBaseInfo {
            alias: alias.into(),
            decimals,
            external_price,
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
            alias: Some(self.alias),
            decimals: Some(self.decimals),
            external_price: self.external_price,
        }
    }
}
