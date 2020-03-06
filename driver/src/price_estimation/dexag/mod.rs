mod api;

use super::{PriceSource, Token};
use crate::models::TokenId;
use anyhow::{anyhow, Result};
use api::{DexagApi, DexagHttpApi};
use futures::future::{self, BoxFuture};
use std::collections::HashMap;

pub struct DexagClient<Api> {
    api: Api,
    // Maps Token::symbol to Token.
    // This is cached in the struct because we don't expect it to change often.
    tokens: HashMap<String, api::Token>,
    stable_coin: api::Token,
}

impl DexagClient<DexagHttpApi> {
    /// Create a DexagClient using DexagHttpApi as the api implementation.
    pub fn new() -> Result<Self> {
        let api = DexagHttpApi::new()?;
        Self::with_api(api)
    }
}

impl<Api> DexagClient<Api>
where
    Api: DexagApi,
{
    pub fn with_api(api: Api) -> Result<Self> {
        // We need to return prices in OWL but Dexag does not track it. OWL tracks
        // USD so we use another stable coin as an approximate USD price.
        const STABLE_COIN: &str = "DAI";

        let tokens = api.get_token_list()?;
        let mut tokens: HashMap<String, api::Token> = tokens
            .into_iter()
            .map(|token| (token.symbol.clone(), token))
            .collect();
        let stable_coin = tokens.remove(STABLE_COIN).ok_or_else(|| {
            anyhow!(
                "dexag exchange does not track our stable coin {}",
                STABLE_COIN
            )
        })?;

        Ok(Self {
            api,
            tokens,
            stable_coin,
        })
    }
}

impl<Api> PriceSource for DexagClient<Api>
where
    Api: DexagApi + Sync,
{
    fn get_prices(&self, tokens: &[Token]) -> Result<HashMap<TokenId, u128>> {
        let (tokens_, futures): (Vec<_>, Vec<_>) = tokens
            .iter()
            .filter_map(|token| -> Option<(&Token, BoxFuture<Result<f64>>)> {
                if token.symbol() == self.stable_coin.symbol {
                    Some((token, Box::pin(future::ready(Ok(1.0)))))
                } else if let Some(api_token) = self.tokens.get(token.symbol()) {
                    Some((token, self.api.get_price(api_token, &self.stable_coin)))
                } else {
                    None
                }
            })
            .unzip();

        let joined = future::join_all(futures);
        let results = futures::executor::block_on(joined);
        assert_eq!(tokens_.len(), results.len());

        Ok(tokens_
            .iter()
            .zip(results.iter())
            .filter_map(|(token, result)| match result {
                Ok(price) => Some((token, price)),
                Err(_) => None,
            })
            .map(|(token, price)| (token.id, token.get_owl_price(*price)))
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::api::MockDexagApi;
    use super::*;
    use mockall::predicate::*;

    #[test]
    fn fails_if_stable_coin_does_not_exist() {
        let mut api = MockDexagApi::new();
        api.expect_get_token_list()
            .returning(move || Ok(Vec::new()));

        assert!(DexagClient::with_api(api).is_err());
    }

    #[test]
    fn get_token_prices() {
        let mut api = MockDexagApi::new();

        let tokens = [
            Token::new(6, "DAI", 18),
            Token::new(1, "ETH", 18),
            Token::new(4, "USDC", 6),
        ];

        let api_tokens = [
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

        let api_tokens_ = api_tokens.clone();
        api.expect_get_token_list()
            .returning(move || Ok(api_tokens_.to_vec()));

        api.expect_get_price()
            .with(eq(api_tokens[1].clone()), eq(api_tokens[0].clone()))
            .returning(|_, _| Box::pin(future::ready(Ok(0.7))));
        api.expect_get_price()
            .with(
                eq(api_tokens[2].clone()),
                #[allow(clippy::redundant_clone)]
                eq(api_tokens[0].clone()),
            )
            .returning(|_, _| Box::pin(future::ready(Ok(1.2))));

        let client = DexagClient::with_api(api).unwrap();
        let prices = client.get_prices(&tokens).unwrap();
        assert_eq!(
            prices,
            hash_map! {
                TokenId(1) => tokens[1].get_owl_price(0.7) as u128,
                TokenId(4) => tokens[2].get_owl_price(1.2) as u128,
                TokenId(6) => tokens[0].get_owl_price(1.0) as u128,
            }
        );
    }

    #[test]
    fn get_token_prices_error() {
        let mut api = MockDexagApi::new();

        let tokens = [Token::new(6, "DAI", 18), Token::new(1, "ETH", 18)];

        let api_tokens = [
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

        let api_tokens_ = api_tokens.clone();
        api.expect_get_token_list()
            .returning(move || Ok(api_tokens_.to_vec()));

        api.expect_get_price()
            .with(
                eq(api_tokens[1].clone()),
                #[allow(clippy::redundant_clone)]
                eq(api_tokens[0].clone()),
            )
            .returning(|_, _| Box::pin(future::ready(Err(anyhow!("")))));

        let client = DexagClient::with_api(api).unwrap();
        let prices = client.get_prices(&tokens).unwrap();
        assert_eq!(
            prices,
            hash_map! {
                // No TokenId(1) because we made the price error above.
                TokenId(6) => tokens[0].get_owl_price(1.0) as u128,
            }
        );
    }

    // Run with `cargo test online_dexag_client -- --include-ignored --nocapture`.
    #[test]
    #[ignore]
    fn online_dexag_client() {
        use std::time::Instant;

        let tokens = &[
            Token::new(1, "WETH", 18),
            Token::new(2, "USDT", 6),
            Token::new(3, "TUSD", 18),
            Token::new(4, "USDC", 6),
            Token::new(5, "PAX", 18),
            Token::new(6, "GUSD", 2),
            Token::new(7, "DAI", 18),
            Token::new(8, "sETH", 18),
            Token::new(9, "sUSD", 18),
            Token::new(15, "SNX", 18),
        ];

        let client = DexagClient::new().unwrap();
        let before = Instant::now();
        let prices = client.get_prices(tokens).unwrap();
        let after = Instant::now();
        println!(
            "Took {} seconds to get prices.",
            (after - before).as_secs_f64()
        );

        for token in tokens {
            if let Some(price) = prices.get(&token.id) {
                println!("Token {} has OWL price of {}.", token.symbol(), price);
            } else {
                println!("Token {} price could not be determined.", token.symbol());
            }
        }
    }
}
