use anyhow::Result;
use core::{
    models::TokenId,
    price_estimation::{average_price_source, price_source::PriceSource},
    token_info::{TokenBaseInfo, TokenInfoFetching},
};
use pricegraph::Pricegraph;
use std::{collections::HashMap, num::NonZeroU128, sync::Arc};
use tokio::sync::{RwLock, RwLockReadGuard};

/// Roughly like `PriceSource` but is updated externally and cannot fail.
#[derive(Debug, Default)]
pub struct PriceCache {
    tokens: HashMap<TokenId, TokenBaseInfo>,
    prices: HashMap<TokenId, NonZeroU128>,
}

impl PriceCache {
    fn update_prices(&mut self, prices: &HashMap<TokenId, NonZeroU128>) {
        self.prices.extend(prices.iter());
    }

    pub fn update_tokens(&mut self, tokens: HashMap<TokenId, TokenBaseInfo>) {
        self.tokens.extend(tokens.into_iter());
    }

    /// Tries to use the current price from the first source, if that fails one base unit of the
    /// token is used (based on decimals) and if that also fails the fee price is used. If we do not
    /// not have the fee price 10e18 is used.
    pub fn price(&self, token_id: TokenId) -> NonZeroU128 {
        match self.prices.get(&token_id) {
            Some(price) => *price,
            None => match self.tokens.get(&token_id) {
                Some(token_info) => token_info.base_unit_in_atoms(),
                None => self
                    .prices
                    .get(&TokenId(0))
                    .copied()
                    .unwrap_or_else(|| NonZeroU128::new(10u128.pow(18)).unwrap()),
            },
        }
    }
}

/// Infallible price source that is updated with the average of external price sources and the
/// pricegraph price source.
pub struct PriceCacheUpdater {
    token_info: Arc<dyn TokenInfoFetching>,
    external_price_sources: Vec<Box<dyn PriceSource + Send + Sync>>,
    inner: RwLock<PriceCache>,
}

impl PriceCacheUpdater {
    pub fn new(
        token_info: Arc<dyn TokenInfoFetching>,
        external_price_sources: Vec<Box<dyn PriceSource + Send + Sync>>,
    ) -> Self {
        Self {
            token_info,
            external_price_sources,
            inner: Default::default(),
        }
    }

    pub async fn inner(&self) -> RwLockReadGuard<'_, PriceCache> {
        self.inner.read().await
    }

    pub async fn update_tokens(&self) -> Result<()> {
        let all_tokens = self.token_info.all_ids().await?;
        let tokens = self.token_info.get_token_infos(&all_tokens).await?;
        self.inner.write().await.update_tokens(tokens);
        Ok(())
    }

    pub async fn update_prices(&self, pricegraph: &Pricegraph) -> Result<()> {
        let all_tokens = self.token_info.all_ids().await?;
        let prices = average_price_source::average_price_sources(
            self.external_price_sources
                .iter()
                .map(|source| source.as_ref() as &(dyn PriceSource + Send + Sync))
                .chain(std::iter::once(
                    pricegraph as &(dyn PriceSource + Send + Sync),
                )),
            &all_tokens,
        )
        .await?;
        self.inner.write().await.update_prices(&prices);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn use_existing_price() {
        let token = TokenId(1);
        let price = NonZeroU128::new(1).unwrap();
        let mut ips = PriceCache::default();
        ips.update_prices(&[(token, price)].iter().copied().collect());
        assert_eq!(ips.price(token), price);
    }

    #[test]
    fn fallback_to_base_unit() {
        let token = TokenId(1);
        let token_info = TokenBaseInfo {
            alias: String::new(),
            decimals: 1,
        };
        let mut ips = PriceCache::default();
        ips.update_tokens([(token, token_info)].iter().cloned().collect());
        assert_eq!(ips.price(token).get(), 10);
    }

    #[test]
    fn fallback_to_fee() {
        let token = TokenId(1);
        let mut ips = PriceCache::default();
        ips.update_prices(
            &[(TokenId(0), NonZeroU128::new(1).unwrap())]
                .iter()
                .copied()
                .collect(),
        );
        assert_eq!(ips.price(token).get(), 1);
    }
}
