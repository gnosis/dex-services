use crate::models::TokenId;
use anyhow::Result;
use futures::future::{BoxFuture, FutureExt as _};
use lazy_static::lazy_static;
use std::{collections::HashMap, num::NonZeroU128};

pub mod cached;
pub mod hardcoded;
pub mod onchain;

pub trait TokenInfoFetching: Send + Sync {
    /// Retrieves some token information from a token ID.
    fn get_token_info<'a>(&'a self, id: TokenId) -> BoxFuture<'a, Result<TokenBaseInfo>>;

    /// Retrieves all token information.
    /// Default implementation calls get_token_info for each token and ignores errors.
    fn get_token_infos<'a>(
        &'a self,
        ids: &'a [TokenId],
    ) -> BoxFuture<'a, Result<HashMap<TokenId, TokenBaseInfo>>> {
        async move {
            let mut result = HashMap::new();
            for id in ids {
                match self.get_token_info(*id).await {
                    Ok(info) => {
                        result.insert(*id, info);
                    }
                    Err(err) => log::warn!("failed to get token info for {}: {:?}", id, err),
                }
            }
            Ok(result)
        }
        .boxed()
    }

    /// Returns a vector with all the token IDs available
    fn all_ids<'a>(&'a self) -> BoxFuture<'a, Result<Vec<TokenId>>>;
}

// mockall workaround https://github.com/asomers/mockall/issues/134
#[cfg(test)]
mod mock {
    use super::*;
    #[mockall::automock]
    pub trait TokenInfoFetching_ {
        fn get_token_info<'a>(&'a self, id: TokenId) -> BoxFuture<'a, Result<TokenBaseInfo>>;
        fn get_token_infos<'a>(
            &'a self,
            ids: &[TokenId],
        ) -> BoxFuture<'a, Result<HashMap<TokenId, TokenBaseInfo>>>;
        fn all_ids<'a>(&'a self) -> BoxFuture<'a, Result<Vec<TokenId>>>;
    }

    impl<T: TokenInfoFetching_ + Send + Sync> TokenInfoFetching for T {
        fn get_token_info<'a>(&'a self, id: TokenId) -> BoxFuture<'a, Result<TokenBaseInfo>> {
            TokenInfoFetching_::get_token_info(self, id)
        }
        fn get_token_infos(
            &self,
            ids: &[TokenId],
        ) -> BoxFuture<Result<HashMap<TokenId, TokenBaseInfo>>> {
            TokenInfoFetching_::get_token_infos(self, ids)
        }
        fn all_ids<'a>(&'a self) -> BoxFuture<'a, Result<Vec<TokenId>>> {
            TokenInfoFetching_::all_ids(self)
        }
    }
}
#[cfg(test)]
pub use mock::MockTokenInfoFetching_ as MockTokenInfoFetching;

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
    pub fn base_unit_in_atoms(&self) -> NonZeroU128 {
        NonZeroU128::new(10u128.pow(self.decimals as u32)).unwrap()
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
    use anyhow::anyhow;

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
        assert_eq!(TokenBaseInfo::new("", 0).base_unit_in_atoms().get(), 1);
        assert_eq!(TokenBaseInfo::new("", 1).base_unit_in_atoms().get(), 10);
        assert_eq!(TokenBaseInfo::new("", 2).base_unit_in_atoms().get(), 100);
    }

    #[test]
    fn default_get_token_infos_forwards_calls_and_ignores_errors() {
        // Not using mockall because we want to test the default impl.
        struct TokenInfo {};
        impl TokenInfoFetching for TokenInfo {
            fn get_token_info<'a>(&'a self, id: TokenId) -> BoxFuture<'a, Result<TokenBaseInfo>> {
                immediate!(match id.0 {
                    0 | 1 => Ok(TokenBaseInfo {
                        alias: id.0.to_string(),
                        decimals: 1
                    }),
                    _ => Err(anyhow!("")),
                })
            }
            fn all_ids<'a>(&'a self) -> BoxFuture<'a, Result<Vec<TokenId>>> {
                unimplemented!()
            }
        }
        let token_info = TokenInfo {};
        let result = token_info
            .get_token_infos(&[TokenId(0), TokenId(1), TokenId(2)])
            .now_or_never()
            .unwrap()
            .unwrap();
        assert_eq!(result.get(&TokenId(0)).unwrap().alias, "0");
        assert_eq!(result.get(&TokenId(1)).unwrap().alias, "1");
        assert!(result.get(&TokenId(2)).is_none());
    }
}
