use super::super::PriceSource;
use crate::http::HttpFactory;
use crate::models::TokenId;
use crate::token_info::{TokenBaseInfo, TokenInfoFetching};
use anyhow::{anyhow, Context, Result};
use futures::{
    future::{self, BoxFuture, FutureExt as _},
    lock::Mutex,
};
use std::num::NonZeroU128;
use std::{any, collections::HashMap, sync::Arc};

/// Provides a generic interface to communicate in a standardized way
/// with specific API token implementations
pub trait GenericToken {
    /// Symbol describing the ERC20 token represented by the type instance
    fn symbol(&self) -> &str;
}

#[cfg_attr(test, mockall::automock(type Token=tests::MockGenericToken;))]
pub trait Api: Sized {
    type Token: GenericToken + Sync + Send;

    /// Creates a new HTTP interface for connecting to the API
    fn bind(http_factory: &HttpFactory) -> Result<Self>;

    fn get_token_list<'a>(&'a self) -> BoxFuture<'a, Result<Vec<Self::Token>>>;

    /// Returns the price of one unit of `from` expressed in `to`.
    /// For example `get_price("ETH", "DAI")` is ~220.
    fn get_price<'a>(&'a self, from: &Self::Token, to: &Self::Token) -> BoxFuture<'a, Result<f64>>;

    /// Returns a string representing the reference coin in the stead of OWL for this API
    /// Could be different from "OWL", e.g., when the API does not offer prices with
    /// respect to OWL
    fn reference_token_symbol() -> &'static str;
}

struct Tokens<T: Api> {
    // Maps uppercase Token::symbol to Token.
    // This is cached in the struct because we don't expect it to change often.
    tokens: HashMap<String, T::Token>,
    stable_coin: T::Token,
}

pub struct GenericClient<T: Api> {
    api: T,
    /// Lazily retrieved the first time it is needed when `get_prices` is
    /// called. We don't want to use the network in `new`.
    api_tokens: Mutex<Option<Tokens<T>>>,
    token_info_fetcher: Arc<dyn TokenInfoFetching>,
}

impl<T: Api> GenericClient<T> {
    /// Create a GenericClient using the api implementation.
    pub fn new(
        http_factory: &HttpFactory,
        token_info_fetcher: Arc<dyn TokenInfoFetching>,
    ) -> Result<Self> {
        let api = T::bind(http_factory)?;
        Ok(Self::with_api_and_tokens(api, token_info_fetcher))
    }

    pub fn with_api_and_tokens(api: T, token_info_fetcher: Arc<dyn TokenInfoFetching>) -> Self {
        Self {
            api,
            api_tokens: Mutex::new(None),
            token_info_fetcher,
        }
    }

    async fn create_api_tokens(&self) -> Result<Tokens<T>> {
        let tokens = self.api.get_token_list().await?;
        let mut tokens: HashMap<String, T::Token> = tokens
            .into_iter()
            .map(|token| (token.symbol().to_uppercase(), token))
            .collect();

        let reference_token_symbol = &T::reference_token_symbol().to_uppercase();
        let stable_coin = tokens
            .remove(reference_token_symbol)
            .ok_or_else(|| anyhow!("exchange does not track {}", reference_token_symbol))?;

        Ok(Tokens {
            tokens,
            stable_coin,
        })
    }
}

type TokenIdAndInfo = (TokenId, TokenBaseInfo);

impl<T> PriceSource for GenericClient<T>
where
    T: Api + Sync + Send,
{
    fn get_prices<'a>(
        &'a self,
        tokens: &'a [TokenId],
    ) -> BoxFuture<'a, Result<HashMap<TokenId, NonZeroU128>>> {
        async move {
            if tokens.is_empty() {
                return Ok(HashMap::new());
            }

            let mut api_tokens_guard = self.api_tokens.lock().await;
            let api_tokens: &Tokens<T> = match api_tokens_guard.as_ref() {
                Some(api_tokens) => api_tokens,
                None => {
                    let initialized = self
                        .create_api_tokens()
                        .await
                        .with_context(|| anyhow!("failed to perform lazy initialization"))?;
                    api_tokens_guard.get_or_insert(initialized)
                }
            };

            let token_infos = self.token_info_fetcher.get_token_infos(tokens).await?;
            let (tokens_, futures): (Vec<TokenIdAndInfo>, Vec<_>) = token_infos
                .into_iter()
                .filter_map(
                    |(token_id, token_info)| -> Option<(TokenIdAndInfo, BoxFuture<Result<f64>>)> {
                        // api_tokens symbols are converted to uppercase to disambiguate
                        let symbol = token_info.symbol().to_uppercase();
                        if symbol == api_tokens.stable_coin.symbol() {
                            Some(((token_id, token_info), immediate!(Ok(1.0))))
                        } else if let Some(api_token) = api_tokens.tokens.get(&symbol) {
                            Some((
                                (token_id, token_info),
                                self.api.get_price(api_token, &api_tokens.stable_coin),
                            ))
                        } else {
                            None
                        }
                    },
                )
                .unzip();

            let joined = future::join_all(futures);
            let results = joined.await;
            assert_eq!(tokens_.len(), results.len());

            Ok(tokens_
                .iter()
                .zip(results.iter())
                .filter_map(|((token_id, token_info), result)| match result {
                    Ok(price) => {
                        let owl_price = token_info.get_owl_price(*price);
                        log::debug!("Fetched price for token {}: {}", token_id, owl_price);
                        Some((*token_id, NonZeroU128::new(owl_price)?))
                    }
                    Err(err) => {
                        log::warn!(
                            "failed to retrieve {} prices for token ID {} ({}): {:?}",
                            any::type_name::<T>(),
                            token_id,
                            token_info.symbol(),
                            err,
                        );
                        None
                    }
                })
                .collect())
        }
        .boxed()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::token_info::hardcoded::{TokenData, TokenInfoOverride};
    use anyhow::anyhow;
    use ethcontract::Address;
    use lazy_static::lazy_static;
    use mockall::{predicate::*, Sequence};
    use std::sync::Once;

    #[derive(Clone, Debug, PartialEq)]
    // Cannot autogenerate with Mockall since the derived traits are needed
    // for testing. GenericToken is a trait that is assigned to the internal
    // token representation of the price source, so the output of `.symbol()`
    // isn't expected to change.
    pub struct MockGenericToken(String);
    impl GenericToken for MockGenericToken {
        fn symbol(&self) -> &str {
            &self.0
        }
    }
    impl From<&str> for MockGenericToken {
        fn from(name: &str) -> Self {
            MockGenericToken(name.to_string())
        }
    }

    // Expectations for static methods are shared between multiple tests.
    // https://docs.rs/mockall/0.6.0/mockall/#static-methods
    // Here we create a shared context once and keep it in memory. Creating
    // new contexts for each test would cause race conditions on execution.
    fn initialize_mockapi_context() {
        static SETUP_ONCE: Once = Once::new();

        SETUP_ONCE.call_once(|| {
            let ctx = MockApi::reference_token_symbol_context();
            ctx.expect().returning(|| &"DAI");
            std::mem::forget(ctx);
        });
    }

    #[test]
    fn fails_if_stable_coin_does_not_exist() {
        initialize_mockapi_context();
        let mut api = MockApi::new();

        api.expect_get_token_list()
            .returning(|| async { Ok(Vec::new()) }.boxed());

        let tokens = hash_map! {
            TokenId::from(6) => TokenInfoOverride::new(Address::from_low_u64_be(0), "DAI", 18, None)
        };
        assert!(GenericClient::<MockApi>::with_api_and_tokens(
            api,
            Arc::new(TokenData::from(tokens))
        )
        .get_prices(&[6.into()])
        .now_or_never()
        .unwrap()
        .is_err());
    }

    #[test]
    fn get_token_prices_initialization_fails_then_works() {
        initialize_mockapi_context();
        let tokens = hash_map! {
            TokenId::from(1) => TokenInfoOverride::new(Address::from_low_u64_be(0), "ETH", 18, None)
        };
        let mut api = MockApi::new();
        let mut seq = Sequence::new();

        api.expect_get_token_list()
            .times(2)
            .in_sequence(&mut seq)
            .returning(|| async { Err(anyhow!("")) }.boxed());

        api.expect_get_token_list()
            .times(1)
            .in_sequence(&mut seq)
            .returning(|| async { Ok(vec!["DAI".into()]) }.boxed());

        let client =
            GenericClient::<MockApi>::with_api_and_tokens(api, Arc::new(TokenData::from(tokens)));
        assert!(client
            .get_prices(&[1.into()])
            .now_or_never()
            .unwrap()
            .is_err());
        assert!(client
            .get_prices(&[1.into()])
            .now_or_never()
            .unwrap()
            .is_err());
        assert!(client
            .get_prices(&[1.into()])
            .now_or_never()
            .unwrap()
            .is_ok());
        assert!(client
            .get_prices(&[1.into()])
            .now_or_never()
            .unwrap()
            .is_ok());
    }

    #[test]
    fn get_token_prices() {
        initialize_mockapi_context();
        let mut api = MockApi::new();

        let address = Address::from_low_u64_be(0);
        let tokens = hash_map! {
            TokenId(6) => TokenInfoOverride::new(address, "DAI", 18, None),
            TokenId(1) => TokenInfoOverride::new(address,"ETH", 18, None),
            TokenId(4) => TokenInfoOverride::new(address,"USDC", 6, None),
        };
        lazy_static! {
            static ref API_TOKENS: [MockGenericToken; 3] =
                ["DAI".into(), "ETH".into(), "USDC".into()];
        }

        api.expect_get_token_list()
            .returning(|| async { Ok(API_TOKENS.to_vec()) }.boxed());

        api.expect_get_price()
            .with(eq(API_TOKENS[1].clone()), eq(API_TOKENS[0].clone()))
            .returning(|_, _| async { Ok(0.7) }.boxed());
        api.expect_get_price()
            .with(
                eq(API_TOKENS[2].clone()),
                #[allow(clippy::redundant_clone)]
                eq(API_TOKENS[0].clone()),
            )
            .returning(|_, _| async { Ok(1.2) }.boxed());

        let client =
            GenericClient::<MockApi>::with_api_and_tokens(api, Arc::new(TokenData::from(tokens)));
        let prices = client
            .get_prices(&[1.into(), 4.into(), 6.into()])
            .now_or_never()
            .unwrap()
            .unwrap();
        assert_eq!(
            prices,
            hash_map! {
                TokenId(1) => nonzero!(0.7e18 as u128),
                TokenId(4) => nonzero!(1.2e30 as u128),
                TokenId(6) => nonzero!(1e18 as u128)
            }
        );
    }

    #[test]
    fn get_token_prices_error() {
        initialize_mockapi_context();
        let mut api = MockApi::new();

        let address = Address::from_low_u64_be(0);
        let tokens = hash_map! {
            TokenId(6) => TokenInfoOverride::new(address, "DAI", 18, None),
            TokenId(1) => TokenInfoOverride::new(address, "ETH", 18, None)
        };

        lazy_static! {
            static ref API_TOKENS: [MockGenericToken; 2] = ["DAI".into(), "ETH".into()];
        }

        api.expect_get_token_list()
            .returning(|| async { Ok(API_TOKENS.to_vec()) }.boxed());

        api.expect_get_price()
            .with(
                eq(API_TOKENS[1].clone()),
                #[allow(clippy::redundant_clone)]
                eq(API_TOKENS[0].clone()),
            )
            .returning(|_, _| async { Err(anyhow!("")) }.boxed());

        let client =
            GenericClient::<MockApi>::with_api_and_tokens(api, Arc::new(TokenData::from(tokens)));
        let prices = client
            .get_prices(&[6.into(), 1.into()])
            .now_or_never()
            .unwrap()
            .unwrap();
        assert_eq!(
            prices,
            hash_map! {
                // No TokenId(1) because we made the price error above.
                TokenId(6) => nonzero!(10u128.pow(18)),
            }
        );
    }

    #[test]
    fn test_case_insensitivity() {
        initialize_mockapi_context();
        let mut api = MockApi::new();

        let address = Address::from_low_u64_be(0);
        let tokens = hash_map! {
            TokenId(6) => TokenInfoOverride::new(address, "dai", 18, None),
            TokenId(1) => TokenInfoOverride::new(address, "ETH", 18, None),
            TokenId(4) => TokenInfoOverride::new(address, "sUSD", 6, None)
        };

        lazy_static! {
            static ref API_TOKENS: [MockGenericToken; 3] =
                ["DAI".into(), "eth".into(), "Susd".into()];
        }

        api.expect_get_token_list()
            .returning(|| async { Ok(API_TOKENS.to_vec()) }.boxed());

        api.expect_get_price()
            .returning(|_, _| async { Ok(1.0) }.boxed());

        let client =
            GenericClient::<MockApi>::with_api_and_tokens(api, Arc::new(TokenData::from(tokens)));
        let prices = client
            .get_prices(&[1.into(), 4.into(), 6.into()])
            .now_or_never()
            .unwrap()
            .unwrap();
        assert_eq!(
            prices,
            hash_map! {
                TokenId(1) => nonzero!(1e18 as u128),
                TokenId(4) => nonzero!(1e30 as u128),
                TokenId(6) => nonzero!(1e18 as u128),
            }
        );
    }

    #[test]
    fn ignores_zero_responses() {
        initialize_mockapi_context();
        let mut api = MockApi::new();

        let address = Address::from_low_u64_be(0);
        let tokens = hash_map! {
            TokenId(6) => TokenInfoOverride::new(address, "DAI", 18, None),
            TokenId(1) => TokenInfoOverride::new(address, "ETH", 18, None)
        };

        lazy_static! {
            static ref API_TOKENS: [MockGenericToken; 2] = ["DAI".into(), "ETH".into()];
        }

        api.expect_get_token_list()
            .returning(|| async { Ok(API_TOKENS.to_vec()) }.boxed());

        api.expect_get_price()
            .with(eq(API_TOKENS[1].clone()), eq(API_TOKENS[0].clone()))
            .returning(|_, _| async { Ok(0.0) }.boxed());

        let client =
            GenericClient::<MockApi>::with_api_and_tokens(api, Arc::new(TokenData::from(tokens)));
        let prices = client
            .get_prices(&[1.into()])
            .now_or_never()
            .unwrap()
            .unwrap();
        assert_eq!(prices, hash_map! {});
    }
}
