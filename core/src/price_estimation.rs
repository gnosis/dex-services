//! Module responsible for aggregating price estimates from various sources to
//! give good price estimates to the solver for better results.

mod average_price_source;
pub mod data;
mod dexag;
mod kraken;
mod orderbook_based;
mod price_source;
mod threaded_price_source;

pub use self::data::TokenData;
use self::dexag::DexagClient;
use self::kraken::KrakenClient;
use crate::{
    http::HttpFactory,
    models::{Order, TokenId, TokenInfo},
};
use anyhow::Result;
use average_price_source::AveragePriceSource;
use futures::future::{BoxFuture, FutureExt as _};
use log::warn;
use price_source::{NoopPriceSource, PriceSource, Token};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::iter;
use std::time::Duration;
use threaded_price_source::ThreadedPriceSource;

/// A type alias for token information map that is passed to the solver.
type Tokens = BTreeMap<TokenId, Option<TokenInfo>>;

/// A trait representing a price oracle that retrieves price estimates for the
/// tokens included in the current orderbook.
#[cfg_attr(test, mockall::automock)]
pub trait PriceEstimating {
    fn get_token_prices<'a>(&'a self, orders: &[Order]) -> BoxFuture<'a, Tokens>;
}

pub struct PriceOracle {
    /// The token data supplied by the environment. This ensures that only
    /// whitelisted tokens get their prices estimated.
    tokens: TokenData,
    /// The price source to use.
    source: Box<dyn PriceSource + Send + Sync>,
}

impl PriceOracle {
    /// Creates a new price oracle from a token whitelist data.
    pub fn new(
        http_factory: &HttpFactory,
        tokens: TokenData,
        update_interval: Duration,
    ) -> Result<Self> {
        let source: Box<dyn PriceSource + Send + Sync> = if tokens.is_empty() {
            Box::new(NoopPriceSource)
        } else {
            let source = AveragePriceSource::new(vec![
                Box::new(KrakenClient::new(http_factory, tokens.clone())?),
                Box::new(DexagClient::new(http_factory, tokens.clone())?),
            ]);
            let (source, _) = ThreadedPriceSource::new(
                tokens.all_tokens_to_estimate_price(),
                source,
                update_interval,
            );
            Box::new(source)
        };

        Ok(PriceOracle { tokens, source })
    }

    #[cfg(test)]
    fn with_source(tokens: TokenData, source: impl PriceSource + Send + Sync + 'static) -> Self {
        PriceOracle {
            tokens,
            source: Box::new(source),
        }
    }

    /// Splits order tokens into a vector of tokens that should be priced based
    /// on the token whitelist and a vector of unpriced token ids.
    ///
    /// Note that all token ids in the returned results are guaranteed to be
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
    async fn get_prices(&self, tokens: &[TokenId]) -> HashMap<TokenId, u128> {
        if tokens.is_empty() {
            return HashMap::new();
        }

        match self.source.get_prices(tokens).await {
            Ok(prices) => prices,
            Err(err) => {
                warn!("failed to retrieve token prices: {}", err);
                HashMap::new()
            }
        }
    }
}

impl PriceEstimating for PriceOracle {
    fn get_token_prices<'a>(&'a self, orders: &[Order]) -> BoxFuture<'a, Tokens> {
        let (tokens_to_price, unpriced_token_ids) = self.split_order_tokens(orders);
        async move {
            let token_ids: Vec<TokenId> = tokens_to_price.iter().map(|t| t.id).collect();
            let prices = self.get_prices(&token_ids).await;

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
        .boxed()
    }
}

#[cfg(test)]
mod tests {
    use super::data::TokenBaseInfo;
    use super::*;
    use anyhow::anyhow;
    use price_source::MockPriceSource;

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
                tokens.sort();
                tokens == [TokenId(1), TokenId(2)]
            })
            .returning(|_| {
                async {
                    Ok(hash_map! {
                        TokenId(2) => 1_000_000_000_000_000_000,
                    })
                }
                .boxed()
            });

        let oracle = PriceOracle::with_source(tokens, source);
        let prices = oracle
            .get_token_prices(&[
                Order::for_token_pair(0, 1),
                Order::for_token_pair(1, 2),
                Order::for_token_pair(2, 3),
                Order::for_token_pair(1, 3),
                Order::for_token_pair(0, 2),
            ])
            .now_or_never()
            .unwrap();

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
            .returning(|_| async { Err(anyhow!("error")) }.boxed());

        let oracle = PriceOracle::with_source(tokens, source);
        let prices = oracle
            .get_token_prices(&[Order::for_token_pair(0, 1)])
            .now_or_never()
            .unwrap();

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
        let prices = oracle.get_token_prices(&[]).now_or_never().unwrap();

        assert_eq!(prices, btree_map! { TokenId(0) => None });
    }

    #[test]
    fn price_oracle_uses_uses_fallback_prices() {
        let tokens = TokenData::from(hash_map! {
            TokenId(7) => TokenBaseInfo::new("DAI", 18, 1_000_000_000_000_000_000, true),
        });

        let mut source = MockPriceSource::new();
        source
            .expect_get_prices()
            .returning(|_| async { Ok(HashMap::new()) }.boxed());

        let oracle = PriceOracle::with_source(tokens, source);
        let prices = oracle
            .get_token_prices(&[Order::for_token_pair(0, 7)])
            .now_or_never()
            .unwrap();

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
            .withf(|tokens| tokens == [2.into()])
            .returning(|_| {
                async {
                    Ok(hash_map! {
                        TokenId(1) => 1_000_000_000_000_000_000,
                        TokenId(2) => 1_000_000_000_000_000_000,
                    })
                }
                .boxed()
            });

        let oracle = PriceOracle::with_source(tokens, source);
        let prices = oracle
            .get_token_prices(&[Order::for_token_pair(1, 2)])
            .now_or_never()
            .unwrap();

        assert_eq!(
            prices,
            btree_map! {
                TokenId(0) => None,
                TokenId(1) => Some(TokenInfo::new("WETH", 18, 0)),
                TokenId(2) => Some(TokenInfo::new("USDT", 6, 1_000_000_000_000_000_000)),
            }
        );
    }
}
