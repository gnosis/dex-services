//! Module responsible for aggregating price estimates from various sources to
//! give good price estimates to the solver for better results.

mod average_price_source;
mod clients;
mod orderbook_based;
pub mod price_source;
mod priority_price_source;
mod threaded_price_source;

use self::clients::{DexagClient, KrakenClient, OneinchClient};
use self::orderbook_based::PricegraphEstimator;
use crate::token_info::{hardcoded::TokenData, TokenInfoFetching};
use crate::{
    http::HttpFactory,
    models::{Order, TokenId, TokenInfo},
    orderbook::StableXOrderBookReading,
};
use anyhow::Result;
use average_price_source::AveragePriceSource;
use futures::future::{BoxFuture, FutureExt as _};
use log::warn;
use price_source::PriceSource;
use priority_price_source::PriorityPriceSource;
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
    token_info_fetcher: Arc<dyn TokenInfoFetching>,
    /// The price source to use.
    source: Box<dyn PriceSource + Send + Sync>,
}

impl PriceOracle {
    /// Creates a new price oracle from a token whitelist data.
    pub fn new(
        http_factory: &HttpFactory,
        orderbook_reader: Arc<dyn StableXOrderBookReading>,
        token_data: TokenData,
        update_interval: Duration,
    ) -> Result<Self> {
        let token_info_fetcher = Arc::new(token_data.clone());
        let (kraken_source, _) = ThreadedPriceSource::new(
            token_info_fetcher.clone(),
            KrakenClient::new(http_factory, token_info_fetcher.clone())?,
            update_interval,
        );
        let (dexag_source, _) = ThreadedPriceSource::new(
            token_info_fetcher.clone(),
            DexagClient::new(http_factory, token_info_fetcher.clone())?,
            update_interval,
        );
        let (oneinch_source, _) = ThreadedPriceSource::new(
            token_info_fetcher.clone(),
            OneinchClient::new(http_factory, token_info_fetcher.clone())?,
            update_interval,
        );
        let averaged_source = Box::new(AveragePriceSource::new(vec![
            Box::new(kraken_source),
            Box::new(dexag_source),
            Box::new(oneinch_source),
            Box::new(PricegraphEstimator::new(orderbook_reader)),
        ]));
        let prioritized_source = Box::new(PriorityPriceSource::new(vec![
            Box::new(token_data),
            averaged_source,
        ]));

        Ok(PriceOracle {
            token_info_fetcher,
            source: prioritized_source,
        })
    }

    #[cfg(test)]
    fn with_source(
        token_info_fetcher: Arc<dyn TokenInfoFetching>,
        source: impl PriceSource + Send + Sync + 'static,
    ) -> Self {
        PriceOracle {
            token_info_fetcher,
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

            let mut tokens = Tokens::new();
            for token_id in token_ids_to_price {
                let token_info = if let Some(price) = prices.get(&token_id) {
                    let mut token_info = TokenInfo {
                        alias: None,
                        decimals: None,
                        external_price: *price,
                    };
                    if let Ok(base_info) = self.token_info_fetcher.get_token_info(token_id).await {
                        token_info.alias = Some(base_info.alias);
                        token_info.decimals = Some(base_info.decimals);
                    }
                    Some(token_info)
                } else {
                    None
                };
                tokens.insert(token_id, token_info);
            }
            tokens
        }
        .boxed()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::token_info::hardcoded::{TokenData, TokenInfoOverride};
    use anyhow::anyhow;
    use price_source::{MockPriceSource, NoopPriceSource};

    #[test]
    fn price_oracle_fetches_token_prices() {
        let tokens = Arc::new(TokenData::from(hash_map! {
            TokenId(1) => TokenInfoOverride::new("WETH", 18, 0),
            TokenId(2) => TokenInfoOverride::new("USDT", 6, 0),
        }));

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
                        TokenId(1) => 0,
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
        let tokens = Arc::new(TokenData::from(hash_map! {
            TokenId(1) => TokenInfoOverride::new("WETH", 18, 0),
        }));

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
                TokenId(1) => None,
            }
        );
    }

    #[test]
    fn price_oracle_always_includes_reference_token() {
        let oracle = PriceOracle::with_source(Arc::new(TokenData::default()), NoopPriceSource {});
        let prices = oracle.get_token_prices(&[]).now_or_never().unwrap();

        assert_eq!(prices, btree_map! { TokenId(0) => None });
    }
}
