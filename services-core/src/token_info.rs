use crate::models::TokenId;
use anyhow::Result;
use ethcontract::Address;
use lazy_static::lazy_static;
use std::{borrow::Borrow, collections::HashMap, num::NonZeroU128};

pub mod cached;
pub mod hardcoded;
pub mod onchain;

#[cfg_attr(test, mockall::automock)]
#[async_trait::async_trait]
pub trait TokenInfoFetching: Send + Sync {
    /// Retrieves some token information from a token ID.
    async fn get_token_info(&self, id: TokenId) -> Result<TokenBaseInfo>;

    /// Retrieves all token information.
    /// Default implementation calls get_token_info for each token and ignores errors.
    async fn get_token_infos(&self, ids: &[TokenId]) -> Result<HashMap<TokenId, TokenBaseInfo>> {
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

    /// Returns a vector with all the token IDs available
    async fn all_ids(&self) -> Result<Vec<TokenId>>;

    /// Retrieves a token by symbol.
    ///
    /// Default implementation queries token info for all IDs and searches the
    /// resulting token for the specified symbol.
    async fn find_token_by_symbol(&self, symbol: &str) -> Result<Option<(TokenId, TokenBaseInfo)>> {
        let infos = self.get_token_infos(&self.all_ids().await?).await?;
        Ok(search_for_token_by_symbol(infos, symbol))
    }

    /// Retrieves a token by address.
    ///
    /// Default implementation queries token info for all IDs and searches the
    /// resulting token for the specified address.
    async fn find_token_by_address(
        &self,
        address: Address,
    ) -> Result<Option<(TokenId, TokenBaseInfo)>> {
        let infos = self.get_token_infos(&self.all_ids().await?).await?;
        Ok(find_token_by_address(infos, address))
    }
}

fn search_for_token_by_symbol<T>(
    tokens: impl IntoIterator<Item = (TokenId, T)>,
    symbol: &str,
) -> Option<(TokenId, T)>
where
    T: Borrow<TokenBaseInfo>,
{
    tokens
        .into_iter()
        .filter(|(_, info)| info.borrow().matches_symbol(symbol))
        .min_by_key(|(id, _)| *id)
}

fn find_token_by_address(
    infos: impl IntoIterator<Item = (TokenId, TokenBaseInfo)>,
    address: Address,
) -> Option<(TokenId, TokenBaseInfo)> {
    infos.into_iter().find(|(_, info)| info.address == address)
}

#[cfg_attr(test, derive(Eq, PartialEq))]
#[derive(Clone, Debug)]
pub struct TokenBaseInfo {
    pub address: Address,
    pub alias: String,
    pub decimals: u8,
}

impl TokenBaseInfo {
    /// Create new token information from its parameters.
    #[cfg(test)]
    pub fn new(address: Address, alias: impl Into<String>, decimals: u8) -> Self {
        TokenBaseInfo {
            address,
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

    /// Returns true if the token alias or symbol matches the speciefied symbol.
    pub fn matches_symbol(&self, symbol: &str) -> bool {
        self.alias == symbol || self.symbol() == symbol
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use anyhow::anyhow;
    use futures::FutureExt as _;

    #[test]
    fn token_get_price() {
        let address = Address::from_low_u64_be(0);
        for (token, usd_price, expected) in &[
            (TokenBaseInfo::new(address, "USDC", 6), 0.99, 0.99e30),
            (TokenBaseInfo::new(address, "DAI", 18), 1.01, 1.01e18),
            (TokenBaseInfo::new(address, "FAKE", 32), 1.0, 1e4),
            (TokenBaseInfo::new(address, "SCAM", 42), 1e10, 1e4),
        ] {
            let owl_price = token.get_owl_price(*usd_price);
            assert_eq!(owl_price, *expected as u128);
        }
    }

    #[test]
    fn token_get_price_without_rounding_error() {
        assert_eq!(
            TokenBaseInfo::new(Address::from_low_u64_be(0), "OWL", 18).get_owl_price(1.0),
            1_000_000_000_000_000_000,
        );
    }

    #[test]
    fn weth_token_symbol_is_eth() {
        assert_eq!(
            TokenBaseInfo::new(Address::from_low_u64_be(0), "WETH", 18).symbol(),
            "ETH"
        );
    }

    #[test]
    #[allow(clippy::float_cmp)]
    fn base_unit_in_atoms() {
        let address = Address::from_low_u64_be(0);
        assert_eq!(
            TokenBaseInfo::new(address, "", 0)
                .base_unit_in_atoms()
                .get(),
            1
        );
        assert_eq!(
            TokenBaseInfo::new(address, "", 1)
                .base_unit_in_atoms()
                .get(),
            10
        );
        assert_eq!(
            TokenBaseInfo::new(address, "", 2)
                .base_unit_in_atoms()
                .get(),
            100
        );
    }

    #[test]
    fn default_get_token_infos_forwards_calls_and_ignores_errors() {
        // Not using mockall because we want to test the default impl.
        struct TokenInfo {};
        #[async_trait::async_trait]
        impl TokenInfoFetching for TokenInfo {
            async fn get_token_info(&self, id: TokenId) -> Result<TokenBaseInfo> {
                match id.0 {
                    0 | 1 => Ok(TokenBaseInfo {
                        address: Address::from_low_u64_be(0),
                        alias: id.0.to_string(),
                        decimals: 1,
                    }),
                    _ => Err(anyhow!("")),
                }
            }
            async fn all_ids(&self) -> Result<Vec<TokenId>> {
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

    #[test]
    fn search_prefers_symbol_of_lower_token_ids() {
        let owl = TokenBaseInfo {
            address: Address::from_low_u64_be(0),
            alias: "OWL".to_owned(),
            decimals: 18,
        };

        let (id, info) = search_for_token_by_symbol(
            vec![
                (TokenId(42), owl.clone()),
                (TokenId(1337), owl.clone()),
                (TokenId(0), owl.clone()),
            ],
            "OWL",
        )
        .unwrap();
        assert_eq!((id, info), (TokenId(0), owl));
    }

    #[test]
    fn find_token_info_by_address_finds_result() {
        let infos = hash_map!(
            TokenId(0) => TokenBaseInfo::new(Address::from_low_u64_be(0), "a", 0),
            TokenId(1) => TokenBaseInfo::new(Address::from_low_u64_be(1), "b", 1),
            TokenId(2) => TokenBaseInfo::new(Address::from_low_u64_be(2), "c", 2),
        );
        let address = Address::from_low_u64_be(1);
        let result = find_token_by_address(infos, address);
        let expected = Some((TokenId(1), TokenBaseInfo::new(address, "b", 1)));
        assert_eq!(result, expected);
    }
}
