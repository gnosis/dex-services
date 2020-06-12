//! Module responsible for aggregating price estimates from various sources to
//! give good price estimates to the solver for better results.

mod average_price_source;
pub mod data;
mod dexag;
mod kraken;
mod orderbook_based;
mod price_source;
mod threaded_price_source;

pub use self::data::{TokenBaseInfo, TokenData};
use self::dexag::DexagClient;
use self::kraken::KrakenClient;
use self::orderbook_based::PricegraphEstimator;
use crate::{
    http::HttpFactory,
    models::{Order, TokenId, TokenInfo},
    orderbook::StableXOrderBookReading,
};
use anyhow::Result;
use average_price_source::AveragePriceSource;
use futures::future::{BoxFuture, FutureExt as _};
use log::warn;
use price_source::{NoopPriceSource, PriceSource};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::iter;
use std::iter::FromIterator;
use std::sync::Arc;
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
        orderbook_reader: Arc<dyn StableXOrderBookReading>,
        tokens: TokenData,
        update_interval: Duration,
    ) -> Result<Self> {
        let source: Box<dyn PriceSource + Send + Sync> = if tokens.is_empty() {
            Box::new(NoopPriceSource)
        } else {
            let (kraken_source, _) = ThreadedPriceSource::new(
                tokens.all_ids(),
                KrakenClient::new(http_factory, tokens.clone())?,
                update_interval,
            );
            let (dexag_source, _) = ThreadedPriceSource::new(
                tokens.all_ids(),
                DexagClient::new(http_factory, tokens.clone())?,
                update_interval,
            );
            Box::new(AveragePriceSource::new(vec![
                Box::new(kraken_source),
                Box::new(dexag_source),
                Box::new(PricegraphEstimator::new(orderbook_reader)),
            ]))
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
        let token_ids_to_price: HashSet<_> = orders
            .iter()
            .flat_map(|order| vec![order.buy_token, order.sell_token])
            .map(TokenId)
            // NOTE: Always include the reference token. This is done since the
            //   solver input specifies the reference token, so for correctness
            //   it should always be considered.
            .chain(iter::once(TokenId::reference()))
            .collect();
        async move {
            let prices = self
                .get_prices(&Vec::from_iter(token_ids_to_price.clone()))
                .await;

            token_ids_to_price
                .into_iter()
                .map(|token_id| {
                    let price = prices.get(&token_id).copied();
                    let token_info = if let Some(base_info) = self.tokens.info(token_id) {
                        Some(TokenInfo {
                            external_price: price.unwrap_or(base_info.external_price),
                            ..base_info.clone().into()
                        })
                    } else if let Some(price) = price {
                        Some(TokenInfo {
                            alias: None,
                            decimals: None,
                            external_price: price,
                        })
                    } else {
                        None
                    };
                    (token_id, token_info)
                })
                .collect()
        }
        .boxed()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use anyhow::anyhow;
    use price_source::MockPriceSource;

    #[test]
    fn price_oracle_fetches_token_prices() {
        let tokens = TokenData::from(hash_map! {
            TokenId(1) => TokenBaseInfo::new("WETH", 18, 0),
            TokenId(2) => TokenBaseInfo::new("USDT", 6, 0),
        });

        let mut source = MockPriceSource::new();
        source
            .expect_get_prices()
            .withf(|tokens| {
                let mut tokens = tokens.to_vec();
                tokens.sort();
                tokens == [TokenId(0), TokenId(1), TokenId(2), TokenId(3)]
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
            TokenId(1) => TokenBaseInfo::new("WETH", 18, 0),
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
        let oracle = PriceOracle::with_source(TokenData::default(), NoopPriceSource {});
        let prices = oracle.get_token_prices(&[]).now_or_never().unwrap();

        assert_eq!(prices, btree_map! { TokenId(0) => None });
    }

    #[test]
    fn price_oracle_uses_uses_fallback_prices() {
        let tokens = TokenData::from(hash_map! {
            TokenId(7) => TokenBaseInfo::new("DAI", 18, 1_000_000_000_000_000_000),
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
}
