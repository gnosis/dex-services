use anyhow::Result;
use futures::future::BoxFuture;
use lazy_static::lazy_static;
#[cfg(test)]
use mockall::automock;
use std::collections::HashMap;

use crate::models::TokenId;
pub mod cached;
pub mod hardcoded;
pub mod onchain;

#[cfg_attr(test, automock)]
pub trait TokenInfoFetching: Send + Sync {
    /// Retrieves some token information from a token ID.
    fn get_token_info<'a>(&'a self, id: TokenId) -> BoxFuture<'a, Result<TokenBaseInfo>>;

    /// Returns a vector with all the token IDs available
    fn all_ids<'a>(&'a self) -> BoxFuture<'a, Result<Vec<TokenId>>>;
}
#[cfg_attr(test, derive(Eq, PartialEq))]
#[derive(Clone, Debug)]
pub struct TokenBaseInfo {
    pub alias: String,
    pub decimals: u8,
}

impl TokenBaseInfo {
    /// Create new token information from its parameters.
    #[cfg(test)]
    pub fn new(alias: impl Into<String>, decimals: u8) -> Self {
        TokenBaseInfo {
            alias: alias.into(),
            decimals,
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

    /// One unit of the token taking decimals into account, given in number of atoms.
    pub fn base_unit_in_atoms(&self) -> u128 {
        10u128.pow(self.decimals as u32)
    }

    /// Converts the prices from USD into the unit expected by the contract.
    /// This price is relative to the OWL token which is considered pegged at
    /// exactly 1 USD with 18 decimals.
    pub fn get_owl_price(&self, usd_price: f64) -> u128 {
        let pow = 36 - (self.decimals as i32);
        (usd_price * 10f64.powi(pow)) as _
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn token_get_price() {
        for (token, usd_price, expected) in &[
            (TokenBaseInfo::new("USDC", 6), 0.99, 0.99e30),
            (TokenBaseInfo::new("DAI", 18), 1.01, 1.01e18),
            (TokenBaseInfo::new("FAKE", 32), 1.0, 1e4),
            (TokenBaseInfo::new("SCAM", 42), 1e10, 1e4),
        ] {
            let owl_price = token.get_owl_price(*usd_price);
            assert_eq!(owl_price, *expected as u128);
        }
    }

    #[test]
    fn token_get_price_without_rounding_error() {
        assert_eq!(
            TokenBaseInfo::new("OWL", 18).get_owl_price(1.0),
            1_000_000_000_000_000_000,
        );
    }

    #[test]
    fn weth_token_symbol_is_eth() {
        assert_eq!(TokenBaseInfo::new("WETH", 18).symbol(), "ETH");
    }

    #[test]
    #[allow(clippy::float_cmp)]
    fn base_unit_in_atoms() {
        assert_eq!(TokenBaseInfo::new("", 0).base_unit_in_atoms(), 1);
        assert_eq!(TokenBaseInfo::new("", 1).base_unit_in_atoms(), 10);
        assert_eq!(TokenBaseInfo::new("", 2).base_unit_in_atoms(), 100);
    }
}
