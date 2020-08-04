use anyhow::Result;
use core::{
    models::TokenId,
    price_estimation::{average_price_source, price_source::PriceSource},
    token_info::TokenBaseInfo,
};
use pricegraph::Pricegraph;
use std::{collections::HashMap, num::NonZeroU128};
use tokio::sync::{RwLock, RwLockReadGuard};

/// Roughly like `PriceSource` but is updated externally and cannot fail.
#[derive(Debug, Default)]
pub struct InfalliblePriceSource {
    token_infos: HashMap<TokenId, TokenBaseInfo>,
    prices: HashMap<TokenId, NonZeroU128>,
}

impl InfalliblePriceSource {
    fn new(token_infos: HashMap<TokenId, TokenBaseInfo>) -> Self {
        Self {
            token_infos,
            prices: HashMap::new(),
        }
    }

    fn update(&mut self, prices: &HashMap<TokenId, NonZeroU128>) {
        self.prices.extend(prices.iter());
    }

    /// Tries to use the current price from the first source, if that fails one base unit of the
    /// token is used (based on decimals) and if that also fails the fee price is used. If we do not
    /// not have the fee price 10e18 is used.
    pub fn price(&self, token_id: TokenId) -> NonZeroU128 {
        match self.prices.get(&token_id) {
            Some(price) => *price,
            None => match self.token_infos.get(&token_id) {
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
#[derive(Default)]
pub struct UpdatingInfalliblePriceSource {
    all_tokens: Vec<TokenId>,
    external_price_sources: Vec<Box<dyn PriceSource + Send + Sync>>,
    inner: RwLock<InfalliblePriceSource>,
}

impl UpdatingInfalliblePriceSource {
    #[allow(dead_code)]
    pub fn new(
        all_tokens: Vec<TokenId>,
        token_infos: HashMap<TokenId, TokenBaseInfo>,
        external_price_sources: Vec<Box<dyn PriceSource + Send + Sync>>,
    ) -> Self {
        Self {
            all_tokens,
            external_price_sources,
            inner: RwLock::new(InfalliblePriceSource::new(token_infos)),
        }
    }

    pub async fn inner(&self) -> RwLockReadGuard<'_, InfalliblePriceSource> {
        self.inner.read().await
    }

    pub async fn update(&self, pricegraph: &Pricegraph) -> Result<()> {
        let prices = average_price_source::average_price_sources(
            self.external_price_sources
                .iter()
                .map(|source| source.as_ref() as &(dyn PriceSource + Send + Sync))
                .chain(std::iter::once(
                    pricegraph as &(dyn PriceSource + Send + Sync),
                )),
            &self.all_tokens,
        )
        .await?;
        self.inner.write().await.update(&prices);
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
        let mut ips = InfalliblePriceSource::default();
        ips.update(&[(token, price)].iter().copied().collect());
        assert_eq!(ips.price(token), price);
    }

    #[test]
    fn fallback_to_base_unit() {
        let token = TokenId(1);
        let token_info = TokenBaseInfo {
            alias: String::new(),
            decimals: 1,
        };
        let ips = InfalliblePriceSource::new([(token, token_info)].iter().cloned().collect());
        assert_eq!(ips.price(token).get(), 10);
    }

    #[test]
    fn fallback_to_fee() {
        let token = TokenId(1);
        let mut ips = InfalliblePriceSource::default();
        ips.update(
            &[(TokenId(0), NonZeroU128::new(1).unwrap())]
                .iter()
                .copied()
                .collect(),
        );
        assert_eq!(ips.price(token).get(), 1);
    }
}
