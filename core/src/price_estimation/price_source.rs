use crate::models::{TokenId, TokenInfo};
use anyhow::Result;
use futures::future::{BoxFuture, FutureExt as _};
use lazy_static::lazy_static;
use std::collections::HashMap;

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

impl Token {
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

        SYMBOL_OVERRIDES
            .get(&self.info.alias)
            .unwrap_or(&self.info.alias)
    }

    /// Converts the prices from USD into the unit expected by the contract.
    /// This price is relative to the OWL token which is considered pegged at
    /// exactly 1 USD with 18 decimals.
    pub fn get_owl_price(&self, usd_price: f64) -> u128 {
        let pow = 36 - (self.info.decimals as i32);
        (usd_price * 10f64.powi(pow)) as _
    }

    /// Creates a new token from its parameters.
    #[cfg(test)]
    pub fn new(id: impl Into<TokenId>, symbol: impl Into<String>, decimals: u8) -> Self {
        Token {
            id: id.into(),
            info: TokenInfo {
                alias: symbol.into(),
                decimals,
                external_price: 0,
            },
        }
    }
}

/// An abstraction around a type that retrieves price estimate from a source
/// such as an exchange.
pub trait PriceSource {
    /// Retrieve current prices relative to the OWL token for the specified
    /// tokens. The OWL token is pegged at 1 USD with 18 decimals. Returns a
    /// sparse price array as being unable to find a price is not considered an
    /// error.
    fn get_prices<'a>(
        &'a self,
        tokens: &'a [TokenId],
    ) -> BoxFuture<'a, Result<HashMap<TokenId, u128>>>;
}

/// A no-op price source that always succeeds and finds no prices.
pub struct NoopPriceSource;

impl PriceSource for NoopPriceSource {
    fn get_prices(&self, _: &[TokenId]) -> BoxFuture<Result<HashMap<TokenId, u128>>> {
        async { Ok(HashMap::new()) }.boxed()
    }
}

// We would like to tag `PriceSource` with `mockall::automock` but mockall does not support the
// lifetime bounds on `tokens`: https://github.com/asomers/mockall/issues/134 . As a workaround
// we create a similar trait with simpler lifetimes on which mockall works.
#[cfg(test)]
mod mock {
    use super::*;
    #[mockall::automock]
    pub trait PriceSource_ {
        fn get_prices<'a>(
            &'a self,
            tokens: &[TokenId],
        ) -> BoxFuture<'a, Result<HashMap<TokenId, u128>>>;
    }

    impl PriceSource for MockPriceSource_ {
        fn get_prices(&self, tokens: &[TokenId]) -> BoxFuture<Result<HashMap<TokenId, u128>>> {
            PriceSource_::get_prices(self, tokens)
        }
    }
}
#[cfg(test)]
pub use mock::MockPriceSource_ as MockPriceSource;
