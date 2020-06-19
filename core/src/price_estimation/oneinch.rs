mod api;

use super::{PriceSource, TokenData};
use crate::http::HttpFactory;
use crate::models::TokenId;
use anyhow::{anyhow, Context, Result};
use api::{OneinchApi, OneinchHttpApi};
use futures::{
    future::{self, BoxFuture, FutureExt as _},
    lock::Mutex,
};
use std::collections::HashMap;

struct ApiTokens {
    // Maps uppercase Token::symbol to Token.
    // This is cached in the struct because we don't expect it to change often.
    tokens: HashMap<String, api::Token>,
    stable_coin: api::Token,
}

pub struct OneinchClient<Api> {
    api: Api,
    /// Lazily retrieved the first time it is needed when `get_prices` is
    /// called. We don't want to use the network in `new`.
    api_tokens: Mutex<Option<ApiTokens>>,
    tokens: TokenData,
}

impl OneinchClient<OneinchHttpApi> {
    /// Create a OneinchClient using OneinchHttpApi as the api implementation.
    pub fn new(http_factory: &HttpFactory, tokens: TokenData) -> Result<Self> {
        let api = OneinchHttpApi::new(http_factory)?;
        Ok(Self::with_api_and_tokens(api, tokens))
    }
}

impl<Api> OneinchClient<Api>
where
    Api: OneinchApi,
{
    pub fn with_api_and_tokens(api: Api, tokens: TokenData) -> Self {
        Self {
            api,
            api_tokens: Mutex::new(None),
            tokens,
        }
    }

    async fn create_api_tokens(&self) -> Result<ApiTokens> {
        let tokens = self.api.get_token_list().await?;
        let mut tokens: HashMap<String, api::Token> = tokens
            .into_iter()
            .map(|token| (token.symbol.to_uppercase(), token))
            .collect();

        // 1inch does track OWL, but all other price sources use some
        // USD-equivalent token to compute the price of other tokens.
        // The price of OWL is far from 1 USD at the time of writing
        // this comment, for consistency we retrieve prices in DAI.
        const STABLE_COIN: &str = "DAI";
        let stable_coin = tokens
            .remove(STABLE_COIN)
            .ok_or_else(|| anyhow!("1inch exchange does not track {}", STABLE_COIN))?;

        Ok(ApiTokens {
            tokens,
            stable_coin,
        })
    }
}

impl<Api> PriceSource for OneinchClient<Api>
where
    Api: OneinchApi + Sync + Send,
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

            let (tokens_, futures): (Vec<TokenId>, Vec<_>) = tokens
                .iter()
                .filter_map(|token| -> Option<(&TokenId, BoxFuture<Result<f64>>)> {
                    // api_tokens symbols are converted to uppercase to disambiguate
                    let symbol = self.tokens.info(*token)?.symbol().to_uppercase();
                    if symbol == api_tokens.stable_coin.symbol {
                        Some((token, async { Ok(1.0) }.boxed()))
                    } else if let Some(api_token) = api_tokens.tokens.get(&symbol) {
                        Some((
                            token,
                            self.api.get_price(api_token, &api_tokens.stable_coin),
                        ))
                    } else {
                        None
                    }
                })
                .unzip();

            let joined = future::join_all(futures);
            let results = joined.await;
            assert_eq!(tokens_.len(), results.len());

            Ok(tokens_
                .iter()
                .zip(results.iter())
                .filter_map(|(token, result)| match result {
                    Ok(price) => Some((*token, self.tokens.info(*token)?.get_owl_price(*price))),
                    Err(_) => None,
                })
                .collect())
        }
        .boxed()
    }
}

#[cfg(test)]
mod tests {
    use super::api::MockOneinchApi;
    use super::*;
    use crate::price_estimation::data::TokenBaseInfo;
    use crate::util::FutureWaitExt as _;
    use lazy_static::lazy_static;
    use mockall::{predicate::*, Sequence};

    #[test]
    fn fails_if_stable_coin_does_not_exist() {
        let mut api = MockOneinchApi::new();
        api.expect_get_token_list()
            .returning(|| async { Ok(Vec::new()) }.boxed());

        let tokens = hash_map! { TokenId::from(6) => TokenBaseInfo::new("DAI", 18, 0)};
        assert!(OneinchClient::with_api_and_tokens(api, tokens.into())
            .get_prices(&[6.into()])
            .now_or_never()
            .unwrap()
            .is_err());
    }

    #[test]
    fn get_token_prices_initialization_fails_then_works() {
        let tokens = hash_map! { TokenId::from(1) => TokenBaseInfo::new("ETH", 18, 0)};
        let mut api = MockOneinchApi::new();
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
                        decimals: 18,
                    }])
                }
                .boxed()
            });

        let client = OneinchClient::with_api_and_tokens(api, tokens.into());
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
        let mut api = MockOneinchApi::new();

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
                    decimals: 18,
                },
                super::api::Token {
                    name: String::new(),
                    symbol: "ETH".to_string(),
                    address: None,
                    decimals: 18,
                },
                super::api::Token {
                    name: String::new(),
                    symbol: "USDC".to_string(),
                    address: None,
                    decimals: 18,
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

        let client = OneinchClient::with_api_and_tokens(api, tokens.clone().into());
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
        let mut api = MockOneinchApi::new();

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
                    decimals: 18,
                },
                super::api::Token {
                    name: String::new(),
                    symbol: "ETH".to_string(),
                    address: None,
                    decimals: 18,
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

        let client = OneinchClient::with_api_and_tokens(api, tokens.clone().into());
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
        let mut api = MockOneinchApi::new();

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
                    decimals: 18,
                },
                super::api::Token {
                    name: String::new(),
                    symbol: "eth".to_string(),
                    address: None,
                    decimals: 18,
                },
                super::api::Token {
                    name: String::new(),
                    symbol: "Susd".to_string(),
                    address: None,
                    decimals: 18,
                },
            ];
        }

        api.expect_get_token_list()
            .returning(|| async { Ok(API_TOKENS.to_vec()) }.boxed());

        api.expect_get_price()
            .returning(|_, _| async { Ok(1.0) }.boxed());

        let client = OneinchClient::with_api_and_tokens(api, tokens.clone().into());
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

    // Run with `cargo test online_oneinch_client -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn online_oneinch_client() {
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
        let mut ids: Vec<TokenId> = tokens.keys().copied().collect();

        let client = OneinchClient::new(&HttpFactory::default(), tokens.clone().into()).unwrap();
        let before = Instant::now();
        let prices = client.get_prices(&ids).wait().unwrap();
        let after = Instant::now();
        println!(
            "Took {} seconds to get prices.",
            (after - before).as_secs_f64()
        );

        ids.sort();
        for id in ids {
            let symbol = tokens.get(&id).unwrap().symbol();
            if let Some(price) = prices.get(&id) {
                println!("Token {} has OWL price of {}.", symbol, price);
            } else {
                println!("Token {} price could not be determined.", symbol);
            }
        }
    }
}
