//! Module responsible for aggregating price estimates from various sources to
//! give good price estimates to the solver for better results.

pub mod data;
mod dexag;
mod kraken;

pub use self::data::TokenData;
use self::kraken::KrakenClient;
use crate::models::{Order, TokenId, TokenInfo};
use anyhow::Result;
use lazy_static::lazy_static;
use log::warn;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::iter;

/// A type alias for token information map that is passed to the solver.
type Tokens = BTreeMap<TokenId, Option<TokenInfo>>;

/// A trait representing a price oracle that retrieves price estimates for the
/// tokens included in the current orderbook.
#[cfg_attr(test, mockall::automock)]
pub trait PriceEstimating {
    fn get_token_prices(&self, orders: &[Order]) -> Tokens;
}

pub struct PriceOracle {
    /// The token data supplied by the environment. This ensures that only
    /// whitelisted tokens get their prices estimated.
    tokens: TokenData,
    /// The price source to use.
    ///
    /// Note that currently only one price source is supported, but more price
    /// source are expected to be added in the future.
    source: Box<dyn PriceSource>,
}

impl PriceOracle {
    /// Creates a new price oracle from a token whitelist data.
    pub fn new(tokens: TokenData) -> Result<Self> {
        Ok(PriceOracle::with_source(tokens, KrakenClient::new()?))
    }

    fn with_source(tokens: TokenData, source: impl PriceSource + 'static) -> Self {
        PriceOracle {
            tokens,
            source: Box::new(source),
        }
    }

    /// Splits order tokens into a vector of tokens that should be priced based
    /// on the token whitelist and a vector of unpriced token ids.
    ///
    /// Note that all token ids in the returned results are garanteed to be
    /// unique.
    fn split_order_tokens(
        &self,
        orders: &[Order],
    ) -> (Vec<Token>, Vec<(TokenId, Option<TokenInfo>)>) {
        let unique_token_ids: HashSet<_> = orders
            .iter()
            .flat_map(|order| vec![order.buy_token, order.sell_token])
            .map(TokenId)
            // NOTE: Always include the reference token. This is done since the
            //   solver input specifies the reference token, so for correctness
            //   it should always be considered.
            .chain(iter::once(TokenId::reference()))
            .collect();

        unique_token_ids.into_iter().fold(
            (Vec::new(), Vec::new()),
            |(mut tokens_to_price, mut unpriced_token_ids), id| {
                match self.tokens.info(id).cloned() {
                    Some(info) if info.should_estimate_price => tokens_to_price.push(Token {
                        id,
                        info: info.into(),
                    }),
                    Some(info) => unpriced_token_ids.push((id, Some(info.into()))),
                    None => unpriced_token_ids.push((id, None)),
                }
                (tokens_to_price, unpriced_token_ids)
            },
        )
    }

    /// Gets price estimates for some tokens
    fn get_prices(&self, tokens: &[Token]) -> HashMap<TokenId, u128> {
        if tokens.is_empty() {
            return HashMap::new();
        }

        match self.source.get_prices(tokens) {
            Ok(prices) => prices,
            Err(err) => {
                warn!("failed to retrieve token prices: {}", err);
                HashMap::new()
            }
        }
    }
}

impl PriceEstimating for PriceOracle {
    fn get_token_prices(&self, orders: &[Order]) -> Tokens {
        let (tokens_to_price, unpriced_token_ids) = self.split_order_tokens(orders);
        let prices = self.get_prices(&tokens_to_price);

        tokens_to_price
            .into_iter()
            .map(|token| {
                let price = prices
                    .get(&token.id)
                    .copied()
                    .unwrap_or(token.info.external_price);
                (
                    token.id,
                    Some(TokenInfo {
                        external_price: price,
                        ..token.info
                    }),
                )
            })
            .chain(unpriced_token_ids.into_iter())
            .collect()
    }
}

/// A token reprensentation.
#[cfg_attr(test, derive(Eq, PartialEq))]
#[derive(Clone, Debug)]
struct Token {
    /// The ID of the token.
    id: TokenId,
    /// The token info for this token including, token symbol and number of
    /// decimals.
    info: TokenInfo,
}

impl Token {
    /// Retrieves the token symbol for this token.
    ///
    /// Note that the token info alias is first checked if it is part of a
    /// symbol override map, and if it is, then that value is used instead. This
    /// allows ERC20 tokens like WETH to be treated as ETH, since exchanges
    /// generally only track prices for the latter.
    fn symbol(&self) -> &str {
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
    fn get_owl_price(&self, usd_price: f64) -> u128 {
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
#[cfg_attr(test, mockall::automock)]
trait PriceSource {
    /// Retrieve current prices relative to the OWL token for the specified
    /// tokens. The OWL token is peged at 1 USD with 18 decimals. Returns a
    /// sparce price array as being unable to find a price is not considered an
    /// error.
    fn get_prices(&self, tokens: &[Token]) -> Result<HashMap<TokenId, u128>>;
}

#[cfg(test)]
mod tests {
    use super::data::TokenBaseInfo;
    use super::*;
    use anyhow::anyhow;

    #[test]
    fn price_oracle_fetches_token_prices() {
        let tokens = TokenData::from(hash_map! {
            TokenId(1) => TokenBaseInfo::new("WETH", 18, 0, true),
            TokenId(2) => TokenBaseInfo::new("USDT", 6, 0, true),
        });

        let mut source = MockPriceSource::new();
        source
            .expect_get_prices()
            .withf(|tokens| {
                let mut tokens = tokens.to_vec();
                tokens.sort_unstable_by_key(|token| token.id);
                tokens == [Token::new(1, "WETH", 18), Token::new(2, "USDT", 6)]
            })
            .returning(|_| {
                Ok(hash_map! {
                    TokenId(2) => 1_000_000_000_000_000_000,
                })
            });

        let oracle = PriceOracle::with_source(tokens, source);
        let prices = oracle.get_token_prices(&[
            Order::for_token_pair(0, 1),
            Order::for_token_pair(1, 2),
            Order::for_token_pair(2, 3),
            Order::for_token_pair(1, 3),
            Order::for_token_pair(0, 2),
        ]);

        assert_eq!(
            prices,
            btree_map! {
                TokenId(0) => None,
                TokenId(1) => Some(TokenInfo::new("WETH", 18, 0)),
                TokenId(2) => Some(TokenInfo::new("USDT", 6, 1_000_000_000_000_000_000)),
                TokenId(3) => None,
            }
        );
    }

    #[test]
    fn price_oracle_ignores_source_error() {
        let tokens = TokenData::from(hash_map! {
            TokenId(1) => TokenBaseInfo::new("WETH", 18, 0, true),
        });

        let mut source = MockPriceSource::new();
        source
            .expect_get_prices()
            .returning(|_| Err(anyhow!("error")));

        let oracle = PriceOracle::with_source(tokens, source);
        let prices = oracle.get_token_prices(&[Order::for_token_pair(0, 1)]);

        assert_eq!(
            prices,
            btree_map! {
                TokenId(0) => None,
                TokenId(1) => Some(TokenInfo::new("WETH", 18, 0)),
            }
        );
    }

    #[test]
    fn price_oracle_always_includes_reference_token() {
        let oracle = PriceOracle::with_source(TokenData::default(), MockPriceSource::new());
        let prices = oracle.get_token_prices(&[]);

        assert_eq!(prices, btree_map! { TokenId(0) => None });
    }

    #[test]
    fn price_oracle_uses_uses_fallback_prices() {
        let tokens = TokenData::from(hash_map! {
            TokenId(7) => TokenBaseInfo::new("DAI", 18, 1_000_000_000_000_000_000, true),
        });

        let mut source = MockPriceSource::new();
        source.expect_get_prices().returning(|_| Ok(HashMap::new()));

        let oracle = PriceOracle::with_source(tokens, source);
        let prices = oracle.get_token_prices(&[Order::for_token_pair(0, 7)]);

        assert_eq!(
            prices,
            btree_map! {
                TokenId(0) => None,
                TokenId(7) => Some(TokenInfo::new("DAI", 18, 1_000_000_000_000_000_000)),
            }
        );
    }

    #[test]
    fn price_oracle_ignores_tokens_not_flagged_for_estimation() {
        let tokens = TokenData::from(hash_map! {
            TokenId(1) => TokenBaseInfo::new("WETH", 18, 0, false),
            TokenId(2) => TokenBaseInfo::new("USDT", 6, 0, true),
        });

        let mut source = MockPriceSource::new();
        source
            .expect_get_prices()
            .withf(|tokens| tokens == [Token::new(2, "USDT", 6)])
            .returning(|_| {
                Ok(hash_map! {
                    TokenId(1) => 1_000_000_000_000_000_000,
                    TokenId(2) => 1_000_000_000_000_000_000,
                })
            });

        let oracle = PriceOracle::with_source(tokens, source);
        let prices = oracle.get_token_prices(&[Order::for_token_pair(1, 2)]);

        assert_eq!(
            prices,
            btree_map! {
                TokenId(0) => None,
                TokenId(1) => Some(TokenInfo::new("WETH", 18, 0)),
                TokenId(2) => Some(TokenInfo::new("USDT", 6, 1_000_000_000_000_000_000)),
            }
        );
    }

    #[test]
    fn token_get_price() {
        for (token, usd_price, expected) in &[
            (Token::new(4, "USDC", 6), 0.99, 0.99 * 10f64.powi(30)),
            (Token::new(7, "DAI", 18), 1.01, 1.01 * 10f64.powi(18)),
            (Token::new(42, "FAKE", 32), 1.0, 10f64.powi(4)),
            (Token::new(99, "SCAM", 42), 10f64.powi(10), 10f64.powi(4)),
        ] {
            let owl_price = token.get_owl_price(*usd_price);
            assert_eq!(owl_price, *expected as u128);
        }
    }

    #[test]
    fn token_get_price_without_rounding_error() {
        assert_eq!(
            Token::new(0, "OWL", 18).get_owl_price(1.0),
            1_000_000_000_000_000_000,
        );
    }

    #[test]
    fn weth_token_symbol_is_eth() {
        assert_eq!(Token::new(1, "WETH", 18).symbol(), "ETH");
    }
}
