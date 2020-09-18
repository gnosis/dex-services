use super::PriceSource;
use crate::models::TokenId;
use anyhow::{anyhow, Result};
use futures::future;
use std::collections::HashMap;
use std::hash::Hash;
use std::num::NonZeroU128;

pub struct AveragePriceSource {
    sources: Vec<Box<dyn PriceSource + Send + Sync>>,
}

impl AveragePriceSource {
    pub fn new(sources: Vec<Box<dyn PriceSource + Send + Sync>>) -> Self {
        Self { sources }
    }
}

#[async_trait::async_trait]
impl PriceSource for AveragePriceSource {
    async fn get_prices(&self, tokens: &[TokenId]) -> Result<HashMap<TokenId, NonZeroU128>> {
        average_price_sources(
            self.sources
                .iter()
                .map(|source| -> &dyn PriceSource { source.as_ref() }),
            tokens,
        )
        .await
    }
}

/// Get the price from each price source and apply `average_prices` to them.
/// Errors if all price sources fail. If some but not all fail then the failure is logged and the
/// failures are not part of the average but no error is returned.
pub async fn average_price_sources<'a>(
    sources: impl Iterator<Item = &'a (impl PriceSource + 'a + ?Sized)>,
    tokens: &[TokenId],
) -> Result<HashMap<TokenId, NonZeroU128>> {
    let futures = future::join_all(sources.map(|s| s.get_prices(tokens))).await;
    let acquired_prices: Vec<_> = futures
        .into_iter()
        .filter_map(|f| match f {
            Ok(prices) => Some(prices),
            Err(err) => {
                log::warn!("Price source failed: {}", err);
                None
            }
        })
        .collect();
    if !acquired_prices.is_empty() {
        Ok(average_prices(acquired_prices))
    } else {
        Err(anyhow!("All price sources failed!"))
    }
}

pub fn average_prices(
    price_maps: Vec<HashMap<TokenId, NonZeroU128>>,
) -> HashMap<TokenId, NonZeroU128> {
    // Lossless merger of the collection of hash maps. That is, putting all
    // available prices for each token into a list to be averaged at the end.
    lossless_merge(price_maps)
        .iter()
        .map(|(token, prices)| {
            (
                *token,
                NonZeroU128::new(
                    prices.iter().map(|p| p.get()).sum::<u128>() / prices.len() as u128,
                )
                .expect("Averaging non-zero number will stay non-zero"),
            )
        })
        .collect()
}

/// Lossless merger of a collection of maps puts all available values into a list for each available key
fn lossless_merge<T: Eq + Hash, U>(map_collection: Vec<HashMap<T, U>>) -> HashMap<T, Vec<U>> {
    let mut gathered_maps: HashMap<T, Vec<U>> = HashMap::new();
    for (key, value) in map_collection.into_iter().flatten() {
        gathered_maps.entry(key).or_default().push(value);
    }
    gathered_maps
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lossless_merge_() {
        let a = hash_map! {
            1 => 1,
            2 => 2,
        };
        let b = hash_map! {
            2 => 1,
            3 => 2
        };
        let res = lossless_merge(vec![a, b]);
        let expected = hash_map! {1 => vec![1], 2 => vec![2, 1], 3 => vec![2]};
        assert_eq!(res, expected);
    }

    #[test]
    fn average_prices_() {
        let p0 = hash_map! {
            TokenId(1) => nonzero!(1),
            TokenId(2) => nonzero!(10),
        };
        let p1 = hash_map! {
            TokenId(1) => nonzero!(3),
            TokenId(2) => nonzero!(10),
        };
        let p2 = hash_map! {
            TokenId(2) => nonzero!(20),
        };

        let result = average_prices(vec![p0, p1, p2]);
        let expected = hash_map! {
            TokenId(1) => nonzero!(2),
            TokenId(2) => nonzero!(13),
        };
        assert_eq!(result, expected);
    }
}
