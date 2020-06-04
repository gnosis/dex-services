use super::{PriceSource, Token};
use crate::models::TokenId;
use anyhow::{anyhow, Result};
use futures::future::{self, BoxFuture, FutureExt as _};
use std::collections::{HashMap, HashSet};
use std::hash::Hash;

pub struct AveragePriceSource {
    sources: Vec<Box<dyn PriceSource + Send + Sync>>,
}

impl AveragePriceSource {
    pub fn new(sources: Vec<Box<dyn PriceSource + Send + Sync>>) -> Self {
        Self { sources }
    }
}

impl PriceSource for AveragePriceSource {
    fn get_prices<'a>(
        &'a self,
        tokens: &'a [Token],
    ) -> BoxFuture<'a, Result<HashMap<TokenId, u128>>> {
        async move {
            let price_futures =
                future::join_all(self.sources.iter().map(|s| s.get_prices(tokens))).await;

            let acquired_prices: Vec<HashMap<TokenId, u128>> = price_futures
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
        .boxed()
    }
}

/// Lossless merger of a collection of maps puts all available values into a list for each available key
pub fn lossless_merge<T: Copy + Eq + Hash + PartialEq, U: Clone + Copy>(
    map_collection: Vec<HashMap<T, U>>,
) -> HashMap<T, Vec<U>> {
    let complete_key_set: HashSet<_> = map_collection.iter().map(|m| m.keys()).flatten().collect();
    let mut gathered_maps: HashMap<T, Vec<U>> = HashMap::new();
    for key in &complete_key_set {
        let available_prices = map_collection
            .iter()
            .filter_map(|map| map.get(key).copied())
            .collect();
        gathered_maps.insert(**key, available_prices);
    }
    gathered_maps
}

fn average_prices(price_maps: Vec<HashMap<TokenId, u128>>) -> HashMap<TokenId, u128> {
    // Lossless merger of the collection of hash maps. That is, putting all
    // available prices for each token into a list to be averaged at the end.
    lossless_merge(price_maps)
        .iter()
        .map(|(token, prices)| (*token, prices.iter().sum::<u128>() / prices.len() as u128))
        .collect()
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
            TokenId(0) => 0,
            TokenId(1) => 1,
            TokenId(2) => 10,
        };
        let p1 = hash_map! {
            TokenId(1) => 3,
            TokenId(2) => 10,
        };
        let p2 = hash_map! {
            TokenId(2) => 20,
        };

        let result = average_prices(vec![p0, p1, p2]);
        let expected = hash_map! {
            TokenId(0) => 0,
            TokenId(1) => 2,
            TokenId(2) => 13,
        };
        assert_eq!(result, expected);
    }
}
