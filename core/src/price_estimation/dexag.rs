mod api;

use super::PriceSource;
use crate::http::HttpFactory;
use crate::models::TokenId;
use crate::token_info::{TokenBaseInfo, TokenInfoFetching};
use anyhow::{anyhow, Context, Result};
use api::{DexagApi, DexagHttpApi};
use futures::{
    future::{self, BoxFuture, FutureExt as _},
    lock::Mutex,
    stream::{self, StreamExt},
};
use std::collections::HashMap;
use std::sync::Arc;

struct ApiTokens {
    // Maps uppercase Token::symbol to Token.
    // This is cached in the struct because we don't expect it to change often.
    tokens: HashMap<String, api::Token>,
    stable_coin: api::Token,
}

pub struct DexagClient<Api> {
    api: Api,
    /// Lazily retrieved the first time it is needed when `get_prices` is
    /// called. We don't want to use the network in `new`.
    api_tokens: Mutex<Option<ApiTokens>>,
    token_info_fetcher: Arc<dyn TokenInfoFetching>,
}

impl DexagClient<DexagHttpApi> {
    /// Create a DexagClient using DexagHttpApi as the api implementation.
    pub fn new(
        http_factory: &HttpFactory,
        token_info_fetcher: Arc<dyn TokenInfoFetching>,
    ) -> Result<Self> {
        let api = DexagHttpApi::new(http_factory)?;
        Ok(Self::with_api_and_tokens(api, token_info_fetcher))
    }
}

impl<Api> DexagClient<Api>
where
    Api: DexagApi,
{
    pub fn with_api_and_tokens(api: Api, token_info_fetcher: Arc<dyn TokenInfoFetching>) -> Self {
        Self {
            api,
            api_tokens: Mutex::new(None),
            token_info_fetcher,
        }
    }

    async fn create_api_tokens(&self) -> Result<ApiTokens> {
        let tokens = self.api.get_token_list().await?;
        let mut tokens: HashMap<String, api::Token> = tokens
            .into_iter()
            .map(|token| (token.symbol.to_uppercase(), token))
            .collect();

        // We need to return prices in OWL but Dexag does not track it. OWL
        // tracks USD so we use another stable coin as an approximate
        // USD price.
        const STABLE_COIN: &str = "DAI";
        let stable_coin = tokens.remove(STABLE_COIN).ok_or_else(|| {
            anyhow!(
                "dexag exchange does not track our stable coin {}",
                STABLE_COIN
            )
        })?;

        Ok(ApiTokens {
            tokens,
            stable_coin,
        })
    }
}

type TokenIdAndInfo = (TokenId, TokenBaseInfo);

impl<Api> PriceSource for DexagClient<Api>
where
    Api: DexagApi + Sync + Send,
{
    fn get_prices<'a>(
        &'a self,
        tokens: &'a [TokenId],
    ) -> BoxFuture<'a, Result<HashMap<TokenId, u128>>> {
        async move {
            if tokens.is_empty() {
                return Ok(HashMap::new());
            }

            let mut api_tokens_guard = self.api_tokens.lock().await;
            let api_tokens_option = &mut api_tokens_guard;
            let api_tokens: &ApiTokens = match api_tokens_option.as_ref() {
                Some(api_tokens) => api_tokens,
                None => {
                    let initialized = self
                        .create_api_tokens()
                        .await
                        .with_context(|| anyhow!("failed to perform lazy initialization"))?;
                    api_tokens_option.get_or_insert(initialized)
                }
            };

            let token_infos: Vec<_> = stream::iter(tokens)
                .filter_map(|token| async move {
                    Some((
                        *token,
                        self.token_info_fetcher.get_token_info(*token).await.ok()?,
                    ))
                })
                .collect()
                .await;

            let (tokens_, futures): (Vec<TokenIdAndInfo>, Vec<_>) = token_infos
                .iter()
                .filter_map(
                    |(token_id, token_info)| -> Option<(TokenIdAndInfo, BoxFuture<Result<f64>>)> {
                        // api_tokens symbols are converted to uppercase to disambiguate
                        let symbol = token_info.symbol().to_uppercase();
                        if symbol == api_tokens.stable_coin.symbol {
                            Some(((*token_id, token_info.clone()), immediate!(Ok(1.0))))
                        } else if let Some(api_token) = api_tokens.tokens.get(&symbol) {
                            Some((
                                (*token_id, token_info.clone()),
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
                .filter_map(|(token, result)| match result {
                    Ok(price) => Some((token.0, token.1.get_owl_price(*price))),
                    Err(_) => None,
                })
                .collect())
        }
        .boxed()
    }
}

#[cfg(test)]
mod tests {
    use super::api::MockDexagApi;
    use super::*;
    use crate::token_info::{hardcoded::TokenData, TokenBaseInfo};
    use crate::util::FutureWaitExt as _;
    use lazy_static::lazy_static;
    use mockall::{predicate::*, Sequence};

    #[test]
    fn fails_if_stable_coin_does_not_exist() {
        let mut api = MockDexagApi::new();
        api.expect_get_token_list()
            .returning(|| async { Ok(Vec::new()) }.boxed());

        let tokens = hash_map! { TokenId::from(6) => TokenBaseInfo::new("DAI", 18, 0)};
        assert!(
            DexagClient::with_api_and_tokens(api, Arc::new(TokenData::from(tokens)))
                .get_prices(&[6.into()])
                .now_or_never()
                .unwrap()
                .is_err()
        );
    }

    #[test]
    fn get_token_prices_initialization_fails_then_works() {
        let tokens = hash_map! { TokenId::from(1) => TokenBaseInfo::new("ETH", 18, 0)};
        let mut api = MockDexagApi::new();
        let mut seq = Sequence::new();

        api.expect_get_token_list()
            .times(2)
            .in_sequence(&mut seq)
            .returning(|| async { Err(anyhow!("")) }.boxed());

        api.expect_get_token_list()
            .times(1)
            .in_sequence(&mut seq)
            .returning(|| {
                async {
                    Ok(vec![super::api::Token {
                        name: String::new(),
                        symbol: "DAI".to_string(),
                        address: None,
                    }])
                }
                .boxed()
            });

        let client = DexagClient::with_api_and_tokens(api, Arc::new(TokenData::from(tokens)));
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
        let mut api = MockDexagApi::new();

        let tokens = hash_map! {
            TokenId(6) => TokenBaseInfo::new("DAI", 18, 0),
            TokenId(1) => TokenBaseInfo::new("ETH", 18, 0),
            TokenId(4) => TokenBaseInfo::new("USDC", 6, 0),
        };

        lazy_static! {
            static ref API_TOKENS: [super::api::Token; 3] = [
                super::api::Token {
                    name: String::new(),
                    symbol: "DAI".to_string(),
                    address: None,
                },
                super::api::Token {
                    name: String::new(),
                    symbol: "ETH".to_string(),
                    address: None,
                },
                super::api::Token {
                    name: String::new(),
                    symbol: "USDC".to_string(),
                    address: None,
                },
            ];
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
            DexagClient::with_api_and_tokens(api, Arc::new(TokenData::from(tokens.clone())));
        let prices = client
            .get_prices(&[1.into(), 4.into(), 6.into()])
            .now_or_never()
            .unwrap()
            .unwrap();
        assert_eq!(
            prices,
            hash_map! {
                TokenId(1) => tokens.get(&1.into()).unwrap().get_owl_price(0.7) as u128,
                TokenId(4) => tokens.get(&4.into()).unwrap().get_owl_price(1.2) as u128,
                TokenId(6) => tokens.get(&6.into()).unwrap().get_owl_price(1.0) as u128
            }
        );
    }

    #[test]
    fn get_token_prices_error() {
        let mut api = MockDexagApi::new();

        let tokens = hash_map! {
            TokenId(6) => TokenBaseInfo::new("DAI", 18, 0),
            TokenId(1) => TokenBaseInfo::new("ETH", 18, 0)
        };

        lazy_static! {
            static ref API_TOKENS: [super::api::Token; 2] = [
                super::api::Token {
                    name: String::new(),
                    symbol: "DAI".to_string(),
                    address: None,
                },
                super::api::Token {
                    name: String::new(),
                    symbol: "ETH".to_string(),
                    address: None,
                },
            ];
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
            DexagClient::with_api_and_tokens(api, Arc::new(TokenData::from(tokens.clone())));
        let prices = client
            .get_prices(&[6.into(), 1.into()])
            .now_or_never()
            .unwrap()
            .unwrap();
        assert_eq!(
            prices,
            hash_map! {
                // No TokenId(1) because we made the price error above.
                TokenId(6) => tokens.get(&6.into()).unwrap().get_owl_price(1.0) as u128,
            }
        );
    }

    #[test]
    fn test_case_insensitivity() {
        let mut api = MockDexagApi::new();

        let tokens = hash_map! {
            TokenId(6) => TokenBaseInfo::new("dai", 18, 0),
            TokenId(1) => TokenBaseInfo::new("ETH", 18, 0),
            TokenId(4) => TokenBaseInfo::new("sUSD", 6, 0)
        };

        lazy_static! {
            static ref API_TOKENS: [super::api::Token; 3] = [
                super::api::Token {
                    name: String::new(),
                    symbol: "DAI".to_string(),
                    address: None,
                },
                super::api::Token {
                    name: String::new(),
                    symbol: "eth".to_string(),
                    address: None,
                },
                super::api::Token {
                    name: String::new(),
                    symbol: "Susd".to_string(),
                    address: None,
                },
            ];
        }

        api.expect_get_token_list()
            .returning(|| async { Ok(API_TOKENS.to_vec()) }.boxed());

        api.expect_get_price()
            .returning(|_, _| async { Ok(1.0) }.boxed());

        let client =
            DexagClient::with_api_and_tokens(api, Arc::new(TokenData::from(tokens.clone())));
        let prices = client
            .get_prices(&[1.into(), 4.into(), 6.into()])
            .now_or_never()
            .unwrap()
            .unwrap();
        assert_eq!(
            prices,
            hash_map! {
                TokenId(1) => tokens.get(&1.into()).unwrap().get_owl_price(1.0) as u128,
                TokenId(4) => tokens.get(&4.into()).unwrap().get_owl_price(1.0) as u128,
                TokenId(6) => tokens.get(&6.into()).unwrap().get_owl_price(1.0) as u128
            }
        );
    }

    // Run with `cargo test online_dexag_client -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn online_dexag_client() {
        use std::time::Instant;

        let tokens = hash_map! {
            TokenId(1) => TokenBaseInfo::new("WETH", 18, 0),
            TokenId(2) => TokenBaseInfo::new("USDT", 6, 0),
            TokenId(3) => TokenBaseInfo::new("TUSD", 18, 0),
            TokenId(4) => TokenBaseInfo::new("USDC", 6, 0),
            TokenId(5) => TokenBaseInfo::new("PAX", 18, 0),
            TokenId(6) => TokenBaseInfo::new("GUSD", 2, 0),
            TokenId(7) => TokenBaseInfo::new("DAI", 18, 0),
            TokenId(8) => TokenBaseInfo::new("sETH", 18, 0),
            TokenId(9) => TokenBaseInfo::new("sUSD", 18, 0),
            TokenId(15) => TokenBaseInfo::new("SNX", 18, 0)
        };
        let ids: Vec<TokenId> = tokens.keys().copied().collect();

        let client = DexagClient::new(
            &HttpFactory::default(),
            Arc::new(TokenData::from(tokens.clone())),
        )
        .unwrap();
        let before = Instant::now();
        let prices = client.get_prices(&ids).wait().unwrap();
        let after = Instant::now();
        println!(
            "Took {} seconds to get prices.",
            (after - before).as_secs_f64()
        );

        for (id, token) in tokens {
            if let Some(price) = prices.get(&id) {
                println!("Token {} has OWL price of {}.", token.symbol(), price);
            } else {
                println!("Token {} price could not be determined.", token.symbol());
            }
        }
    }
}
