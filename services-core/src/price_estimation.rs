//! Module responsible for aggregating price estimates from various sources to
//! give good price estimates to the solver for better results.

pub mod average_price_source;
mod clients;
mod orderbook_based;
pub mod price_source;
mod priority_price_source;
mod threaded_price_source;

use self::clients::{DexagClient, KrakenClient, OneinchClient};
use self::orderbook_based::PricegraphEstimator;
use crate::contracts::stablex_contract::StableXContractImpl;
use crate::token_info::{cached::TokenInfoCache, hardcoded::TokenData, TokenInfoFetching};
use crate::{
    economic_viability::NativeTokenPricing,
    http::HttpFactory,
    models::{Order, TokenId, TokenInfo},
    orderbook::StableXOrderBookReading,
};
use anyhow::Result;
use average_price_source::AveragePriceSource;
use log::warn;
use price_source::PriceSource;
use priority_price_source::PriorityPriceSource;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::iter;
use std::iter::FromIterator;
use std::num::NonZeroU128;
use std::sync::Arc;
use std::time::Duration;
use threaded_price_source::ThreadedPriceSource;

/// A type alias for token information map that is passed to the solver.
type Tokens = BTreeMap<TokenId, Option<TokenInfo>>;

/// A trait representing a price oracle that retrieves price estimates for the
/// tokens included in the current orderbook.
#[cfg_attr(test, mockall::automock)]
#[async_trait::async_trait]
pub trait PriceEstimating {
    /// Retrieves price estimates for all token in the specified orders on a
    /// best-effort basis. Critically, it is not considered an error to not be
    /// able to retrieve a price estimate for a given token.
    ///
    /// Prices are in the same format as is expected by the Smart Contract and
    /// the solver, so the amount of OWL in atoms to purchase 1e18 of the
    /// corresponding token.
    async fn get_token_prices(&self, orders: &[Order]) -> Tokens;
}

pub struct PriceOracle {
    /// The token data supplied by the environment. This ensures that only
    /// whitelisted tokens get their prices estimated.
    token_info_fetcher: Arc<dyn TokenInfoFetching>,
    /// The price source to use.
    source: Box<dyn PriceSource + Send + Sync>,
    /// The id of the token in which network transactions fees are paid
    native_token: TokenId,
}

impl PriceOracle {
    /// Creates a new price oracle from a token whitelist data.
    pub fn new(
        http_factory: &HttpFactory,
        orderbook_reader: Arc<dyn StableXOrderBookReading>,
        contract: Arc<StableXContractImpl>,
        token_data: TokenData,
        update_interval: Duration,
        native_token: TokenId,
        use_external_price_source: bool,
    ) -> Result<Self> {
        let cache: HashMap<_, _> = token_data.clone().into();
        let token_info_fetcher = Arc::new(TokenInfoCache::with_cache(contract, cache));
        let mut price_sources: Vec<Box<dyn PriceSource + Send + Sync>> = vec![Box::new(PricegraphEstimator::new(orderbook_reader))];
        if use_external_price_source {
            price_sources.extend(external_price_sources(http_factory, token_info_fetcher.clone(), update_interval)?);
        }
        let averaged_source = Box::new(AveragePriceSource::new(price_sources));
        let prioritized_source = Box::new(PriorityPriceSource::new(vec![
            Box::new(token_data),
            averaged_source,
        ]));

        Ok(PriceOracle {
            token_info_fetcher,
            source: prioritized_source,
            native_token,
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
            native_token: TokenId(1),
        }
    }

    /// Gets price estimates for some tokens
    async fn get_prices(&self, tokens: &[TokenId]) -> HashMap<TokenId, NonZeroU128> {
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

#[async_trait::async_trait]
impl PriceEstimating for PriceOracle {
    async fn get_token_prices(&self, orders: &[Order]) -> Tokens {
        let token_ids_to_price: HashSet<_> = orders
            .iter()
            .flat_map(|order| vec![order.buy_token, order.sell_token])
            .map(TokenId)
            // NOTE: Always include the reference token. This is done since the
            //   solver input specifies the reference token, so for correctness
            //   it should always be considered.
            .chain(iter::once(TokenId::reference()))
            .collect();
        let token_ids_to_price = Vec::<_>::from_iter(token_ids_to_price);
        let prices = self.get_prices(&token_ids_to_price).await;
        let mut token_infos = self
            .token_info_fetcher
            .get_token_infos(&token_ids_to_price)
            .await
            .unwrap_or_default();

        let mut tokens = Tokens::new();
        for token_id in token_ids_to_price {
            let token_info = if let Some(price) = prices.get(&token_id) {
                let mut token_info = TokenInfo {
                    alias: None,
                    decimals: None,
                    external_price: *price,
                };
                if let Some(base_info) = token_infos.remove(&token_id) {
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
}

#[async_trait::async_trait]
impl NativeTokenPricing for PriceOracle {
    async fn get_native_token_price(&self) -> Option<NonZeroU128> {
        let prices = self.source.get_prices(&[self.native_token]).await.ok()?;
        prices.get(&self.native_token).copied()
    }
}

/// Create the external price sources used by PriceOracle.
pub fn external_price_sources(
    http_factory: &HttpFactory,
    token_info_fetcher: Arc<dyn TokenInfoFetching>,
    update_interval: Duration,
) -> Result<Vec<Box<dyn PriceSource + Send + Sync>>> {
    let kraken = KrakenClient::new(http_factory, token_info_fetcher.clone())?;
    let dexag = DexagClient::new(http_factory, token_info_fetcher.clone())?;
    let oneinch = OneinchClient::new(http_factory, token_info_fetcher.clone())?;
    Ok(vec![
        thread_and_box(kraken, token_info_fetcher.clone(), update_interval),
        thread_and_box(dexag, token_info_fetcher.clone(), update_interval),
        thread_and_box(oneinch, token_info_fetcher, update_interval),
    ])
}

fn thread_and_box(
    price_source: impl PriceSource + Send + Sync + 'static,
    token_info_fetcher: Arc<dyn TokenInfoFetching>,
    update_interval: Duration,
) -> Box<dyn PriceSource + Send + Sync> {
    Box::new(ThreadedPriceSource::new(token_info_fetcher, price_source, update_interval).0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::token_info::hardcoded::{TokenData, TokenInfoOverride};
    use anyhow::anyhow;
    use ethcontract::Address;
    use futures::FutureExt as _;
    use price_source::{MockPriceSource, NoopPriceSource};

    #[test]
    fn price_oracle_fetches_token_prices() {
        let address = Address::from_low_u64_be(0);
        let tokens = Arc::new(TokenData::from(hash_map! {
            TokenId(1) => TokenInfoOverride::new(address, "WETH", 18, None),
            TokenId(2) => TokenInfoOverride::new(address, "USDT", 6, None),
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
                Ok(hash_map! {
                    TokenId(1) => nonzero!(100_000),
                    TokenId(2) => nonzero!(1_000_000_000_000_000_000),
                })
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
                TokenId(1) => Some(TokenInfo::new("WETH", 18, 100_000)),
                TokenId(2) => Some(TokenInfo::new("USDT", 6, 1_000_000_000_000_000_000)),
                TokenId(3) => None,
            }
        );
    }

    #[test]
    fn price_oracle_ignores_source_error() {
        let tokens = Arc::new(TokenData::from(hash_map! {
            TokenId(1) => TokenInfoOverride::new(Address::from_low_u64_be(0), "WETH", 18, None),
        }));

        let mut source = MockPriceSource::new();
        source
            .expect_get_prices()
            .returning(|_| Err(anyhow!("error")));

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
