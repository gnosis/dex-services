use super::super::{PriceSource, TokenData};
use crate::http::HttpFactory;
use crate::models::TokenId;
use anyhow::{anyhow, Context, Result};
use futures::{
    future::{self, BoxFuture, FutureExt as _},
    lock::Mutex,
};
use std::collections::HashMap;

pub trait Symbolic {
    fn symbol(&self) -> &str;
}

#[cfg(test)]
#[derive(Clone, Debug, PartialEq)]
// Cannot autogenerate with Mockall since the derived traits are needed
// for testing. Symbolic is a trait that is assigned to the internal token
// representation of the price source, so the output of `.symbol()` isn't
// expected to change.
pub struct MockSymbolic(String);
#[cfg(test)]
impl Symbolic for MockSymbolic {
    fn symbol(&self) -> &str {
        &self.0
    }
}
#[cfg(test)]
impl MockSymbolic {
    fn create(name: &str) -> MockSymbolic {
        MockSymbolic(name.to_string())
    }
}

#[cfg_attr(test, mockall::automock(type Token=MockSymbolic;))]
pub trait Api: Sized {
    type Token: Symbolic + Sync + Send;

    /// Creates a new HTTP interface for connecting to the API
    fn bind(http_factory: &HttpFactory) -> Result<Self>;

    fn get_token_list<'a>(&'a self) -> BoxFuture<'a, Result<Vec<Self::Token>>>;
    /// Returns the price of one unit of `from` expressed in `to`.
    /// For example `get_price("ETH", "DAI")` is ~220.
    fn get_price<'a>(&'a self, from: &Self::Token, to: &Self::Token) -> BoxFuture<'a, Result<f64>>;
}

struct Tokens<T: Api> {
    // Maps uppercase Token::symbol to Token.
    // This is cached in the struct because we don't expect it to change often.
    tokens: HashMap<String, T::Token>,
    stable_coin: T::Token,
}

pub struct Client<T: Api> {
    api: T,
    /// Lazily retrieved the first time it is needed when `get_prices` is
    /// called. We don't want to use the network in `new`.
    api_tokens: Mutex<Option<Tokens<T>>>,
    tokens: TokenData,
}

impl<T: Api> Client<T> {
    /// Create a Client using the api implementation from HttpConnecting.
    pub fn new(http_factory: &HttpFactory, tokens: TokenData) -> Result<Self> {
        let api = T::bind(http_factory)?;
        Ok(Self::with_api_and_tokens(api, tokens))
    }

    pub fn with_api_and_tokens(api: T, tokens: TokenData) -> Self {
        Self {
            api,
            api_tokens: Mutex::new(None),
            tokens,
        }
    }

    async fn create_api_tokens(&self) -> Result<Tokens<T>> {
        let tokens = self.api.get_token_list().await?;
        let mut tokens: HashMap<String, T::Token> = tokens
            .into_iter()
            .map(|token| (token.symbol().to_uppercase(), token))
            .collect();

        // We need to return prices in OWL but some price sources do not track it.
        // OWL tracks USD so we use another stable coin as an approximate USD price.
        const STABLE_COIN: &str = "DAI";
        let stable_coin = tokens
            .remove(STABLE_COIN)
            .ok_or_else(|| anyhow!("exchange does not track {}", STABLE_COIN))?;

        Ok(Tokens {
            tokens,
            stable_coin,
        })
    }
}

impl<T> PriceSource for Client<T>
where
    T: Api + Sync + Send,
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
            let api_tokens: &Tokens<T> = match api_tokens_option.as_ref() {
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
                    if symbol == api_tokens.stable_coin.symbol() {
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
    use super::*;
    use crate::models::TokenId;
    use crate::price_estimation::data::TokenBaseInfo;
    use anyhow::anyhow;
    use lazy_static::lazy_static;
    use mockall::{predicate::*, Sequence};

    #[test]
    fn fails_if_stable_coin_does_not_exist() {
        let mut api = MockApi::new();
        api.expect_get_token_list()
            .returning(|| async { Ok(Vec::new()) }.boxed());

        let tokens = hash_map! { TokenId::from(6) => TokenBaseInfo::new("DAI", 18, 0)};
        assert!(Client::<MockApi>::with_api_and_tokens(api, tokens.into())
            .get_prices(&[6.into()])
            .now_or_never()
            .unwrap()
            .is_err());
    }

    #[test]
    fn get_token_prices_initialization_fails_then_works() {
        let tokens = hash_map! { TokenId::from(1) => TokenBaseInfo::new("ETH", 18, 0)};
        let mut api = MockApi::new();
        let mut seq = Sequence::new();

        api.expect_get_token_list()
            .times(2)
            .in_sequence(&mut seq)
            .returning(|| async { Err(anyhow!("")) }.boxed());

        api.expect_get_token_list()
            .times(1)
            .in_sequence(&mut seq)
            .returning(|| async { Ok(vec![MockSymbolic::create("DAI")]) }.boxed());

        let client = Client::<MockApi>::with_api_and_tokens(api, tokens.into());
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
        let mut api = MockApi::new();

        let tokens = hash_map! {
            TokenId(6) => TokenBaseInfo::new("DAI", 18, 0),
            TokenId(1) => TokenBaseInfo::new("ETH", 18, 0),
            TokenId(4) => TokenBaseInfo::new("USDC", 6, 0),
        };

        lazy_static! {
            static ref API_TOKENS: [MockSymbolic; 3] = [
                MockSymbolic::create("DAI"),
                MockSymbolic::create("ETH"),
                MockSymbolic::create("USDC")
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

        let client = Client::<MockApi>::with_api_and_tokens(api, tokens.clone().into());
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
        let mut api = MockApi::new();

        let tokens = hash_map! {
            TokenId(6) => TokenBaseInfo::new("DAI", 18, 0),
            TokenId(1) => TokenBaseInfo::new("ETH", 18, 0)
        };

        lazy_static! {
            static ref API_TOKENS: [MockSymbolic; 2] =
                [MockSymbolic::create("DAI"), MockSymbolic::create("ETH"),];
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

        let client = Client::<MockApi>::with_api_and_tokens(api, tokens.clone().into());
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
        let mut api = MockApi::new();

        let tokens = hash_map! {
            TokenId(6) => TokenBaseInfo::new("dai", 18, 0),
            TokenId(1) => TokenBaseInfo::new("ETH", 18, 0),
            TokenId(4) => TokenBaseInfo::new("sUSD", 6, 0)
        };

        lazy_static! {
            static ref API_TOKENS: [MockSymbolic; 3] = [
                MockSymbolic::create("DAI"),
                MockSymbolic::create("eth"),
                MockSymbolic::create("Susd"),
            ];
        }

        api.expect_get_token_list()
            .returning(|| async { Ok(API_TOKENS.to_vec()) }.boxed());

        api.expect_get_price()
            .returning(|_, _| async { Ok(1.0) }.boxed());

        let client = Client::<MockApi>::with_api_and_tokens(api, tokens.clone().into());
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
}
