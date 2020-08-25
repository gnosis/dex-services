use super::{TokenBaseInfo, TokenId, TokenInfoFetching};
use anyhow::{anyhow, Context as _, Error, Result};
use async_std::sync::RwLock;
use ethcontract::errors::{ExecutionError, MethodError};
use futures::{
    future::{BoxFuture, FutureExt},
    stream::{self, StreamExt as _},
};
use std::collections::HashMap;
use std::sync::Arc;

/// Default number of concurrent requests used for caching.
pub const DEFAULT_CACHE_CONCURRENT_REQUESTS: usize = 10;

/// Implementation of TokenInfoFetching that stores previously fetched information in an in-memory cache for fast retrieval.
/// TokenIds will always be fetched from the inner layer, as new tokens could be added at any time.
pub struct TokenInfoCache {
    cache: RwLock<HashMap<TokenId, CacheEntry>>,
    inner: Arc<dyn TokenInfoFetching>,
}

#[derive(Debug)]
enum CacheEntry {
    TokenBaseInfo(TokenBaseInfo),
    /// For contract calls that revert. In this case we are unlikely to ever be able to get the
    /// token info so it does not make sense to keep retrying.
    UnretryableError(String),
}

impl TokenInfoCache {
    pub fn new(inner: Arc<dyn TokenInfoFetching>) -> Self {
        Self {
            cache: RwLock::new(HashMap::new()),
            inner,
        }
    }

    #[allow(dead_code)]
    pub fn with_cache(
        inner: Arc<dyn TokenInfoFetching>,
        cache: impl IntoIterator<Item = (TokenId, TokenBaseInfo)>,
    ) -> Self {
        Self {
            inner,
            cache: RwLock::new(
                cache
                    .into_iter()
                    .map(|(key, value)| (key, CacheEntry::TokenBaseInfo(value)))
                    .collect(),
            ),
        }
    }

    /// Attempt to retrieve and cache all token info that is not already cached.
    /// Fails if `all_ids` fails. Does not fail if individual token infos fail.
    ///
    /// This method uses `DEFAULT_CACHE_CONCURRENT_REQUESTS` concurrent requests
    /// for retrieving token data. Use `cache_all_with_concurrent_requests` to
    /// manually specify a number of concurrent requests.
    pub async fn cache_all(&self) -> Result<()> {
        self.cache_all_with_concurrent_requests(DEFAULT_CACHE_CONCURRENT_REQUESTS)
            .await
    }

    /// Attemts to cache all tokens using the specified number of concurrent
    /// requests. This method is identical to `cache_all` except that the number
    /// of concurrent requests may be manually specified.
    pub async fn cache_all_with_concurrent_requests(
        &self,
        number_of_parallel_requests: usize,
    ) -> Result<()> {
        let ids = self.all_ids().await.context("failed to get all ids")?;
        stream::iter(self.uncached_tokens(&ids).await)
            .for_each_concurrent(number_of_parallel_requests, |token_id| async move {
                // Individual tokens might not conform to erc20 in which case we are unable to retrieve
                // their info.
                if let Err(err) = self.get_token_info(token_id).await {
                    log::warn!(
                        "failed to get token info for token id {}: {}",
                        token_id.0,
                        err
                    );
                }
            })
            .await;
        Ok(())
    }

    async fn uncached_tokens(&self, ids: impl IntoIterator<Item = &TokenId>) -> Vec<TokenId> {
        let cache = self.cache.read().await;
        ids.into_iter()
            .copied()
            .filter(|id| !cache.contains_key(id))
            // NOTE: Make sure to `collect` to not hold the `cache` lock.
            .collect()
    }

    async fn find_cached_token_by_symbol(&self, symbol: &str) -> Option<(TokenId, TokenBaseInfo)> {
        let cache = self.cache.read().await;
        let (id, info) = super::search_for_token_by_symbol(
            cache.iter().filter_map(|(id, entry)| match entry {
                CacheEntry::TokenBaseInfo(info) => Some((*id, info)),
                _ => None,
            }),
            symbol,
        )?;

        Some((id, info.clone()))
    }
}

impl TokenInfoFetching for TokenInfoCache {
    fn get_token_info<'a>(&'a self, id: TokenId) -> BoxFuture<'a, Result<TokenBaseInfo>> {
        async move {
            if let Some(entry) = self.cache.read().await.get(&id) {
                return cache_entry_to_result(entry);
            }

            let info = self.inner.get_token_info(id).await;
            match info {
                Ok(info) => {
                    self.cache
                        .write()
                        .await
                        .insert(id, CacheEntry::TokenBaseInfo(info.clone()));
                    Ok(info)
                }
                Err(err) if is_revert(&err) => {
                    log::debug!("unretryable error: {:?}", err);
                    self.cache
                        .write()
                        .await
                        .insert(id, CacheEntry::UnretryableError(err.to_string()));
                    Err(err)
                }
                Err(err) => Err(err),
            }
        }
        .boxed()
    }

    fn get_token_infos<'a>(
        &'a self,
        ids: &'a [TokenId],
    ) -> BoxFuture<'a, Result<HashMap<TokenId, TokenBaseInfo>>> {
        async move {
            let uncached_token_ids = self.uncached_tokens(ids).await;
            // Insert the missing infos into the cache.
            // It would be nice to be able to use `self.inner.get_token_infos` as an optimization
            // but with the current signature of `get_token_infos` we wouldn't be able to
            // access to the individual errors making it impossible to discern unretryable errors.
            for id in uncached_token_ids {
                let _ = self.get_token_info(id).await;
            }

            let cache = self.cache.read().await;
            let result = ids
                .iter()
                .filter_map(|id| {
                    let entry = cache.get(id)?;
                    let result = cache_entry_to_result(entry);
                    let info = result.ok()?;
                    Some((*id, info))
                })
                .collect();
            Ok(result)
        }
        .boxed()
    }

    fn all_ids<'a>(&'a self) -> BoxFuture<'a, Result<Vec<TokenId>>> {
        self.inner.all_ids()
    }

    fn find_token_by_symbol<'a>(
        &'a self,
        symbol: &'a str,
    ) -> BoxFuture<'a, Result<Option<(TokenId, TokenBaseInfo)>>> {
        async move {
            if let Some((id, _)) = self.find_cached_token_by_symbol(symbol).await {
                // NOTE: In case we found a symbol, make sure that all tokens up
                // to that ID are already cached. This ensures that if we find a
                // token with the symbol, it is indeed the one with the lowest
                // token ID on the exchange. Also, if the cache is already warm,
                // this this operation will complete very fast without having to
                // query the inner `TokenInfoFetching`.
                let ids = (0..id.0).map(TokenId).collect::<Vec<_>>();
                self.get_token_infos(&ids).await?;
            } else {
                // NOTE: Token not found - update the entire token cache.
                self.cache_all().await?;
            }

            Ok(self.find_cached_token_by_symbol(symbol).await)
        }
        .boxed()
    }
}

fn cache_entry_to_result(entry: &CacheEntry) -> Result<TokenBaseInfo> {
    match entry {
        CacheEntry::TokenBaseInfo(info) => Ok(info.clone()),
        CacheEntry::UnretryableError(reason) => {
            Err(anyhow!(reason.clone()).context("cached error"))
        }
    }
}

fn is_revert(err: &Error) -> bool {
    matches!(
        err.downcast_ref::<MethodError>(),
        Some(MethodError {
            inner: ExecutionError::Revert(_),
            ..
        })
    )
}

#[cfg(test)]
mod tests {
    use super::super::MockTokenInfoFetching;
    use super::*;
    use anyhow::anyhow;
    use ethcontract::Address;
    use mockall::predicate::eq;

    fn revert_error() -> Error {
        MethodError {
            signature: String::new(),
            inner: ExecutionError::Revert(None),
        }
        .into()
    }

    #[test]
    fn calls_inner_once_per_token_id_on_success() {
        let mut inner = MockTokenInfoFetching::new();

        inner.expect_get_token_info().times(1).returning(|_| {
            immediate!(Ok(TokenBaseInfo {
                address: Address::from_low_u64_be(0),
                alias: "Foo".to_owned(),
                decimals: 18,
            }))
        });

        let cache = TokenInfoCache::new(Arc::new(inner));
        let first = cache
            .get_token_info(1.into())
            .now_or_never()
            .expect("First fetch not immediate")
            .expect("First fetch failed");
        let second = cache
            .get_token_info(1.into())
            .now_or_never()
            .expect("Second fetch not immediate")
            .expect("Second fetch failed");
        assert_eq!(first, second);
    }

    #[test]
    fn calls_inner_again_on_error() {
        let mut inner = MockTokenInfoFetching::new();

        inner
            .expect_get_token_info()
            .times(2)
            .returning(|_| immediate!(Err(anyhow!("error"))));

        let cache = TokenInfoCache::new(Arc::new(inner));
        cache
            .get_token_info(1.into())
            .now_or_never()
            .expect("First fetch not immediate")
            .expect_err("Fetch should return error");
        cache
            .get_token_info(1.into())
            .now_or_never()
            .expect("Second fetch not immediate")
            .expect_err("Fetch should return error");
    }

    #[test]
    fn does_not_call_inner_again_on_revert_error() {
        let mut inner = MockTokenInfoFetching::new();

        inner
            .expect_get_token_info()
            .times(1)
            .returning(|_| immediate!(Err(revert_error())));

        let cache = TokenInfoCache::new(Arc::new(inner));
        cache
            .get_token_info(1.into())
            .now_or_never()
            .expect("First fetch not immediate")
            .expect_err("Fetch should return error");
        cache
            .get_token_info(1.into())
            .now_or_never()
            .expect("Second fetch not immediate")
            .expect_err("Fetch should return error");
    }

    #[test]
    fn always_calls_all_ids_on_inner() {
        let mut inner = MockTokenInfoFetching::new();

        inner
            .expect_all_ids()
            .times(2)
            .returning(|| immediate!(Ok(vec![])));

        let cache = TokenInfoCache::new(Arc::new(inner));
        cache
            .all_ids()
            .now_or_never()
            .expect("Not Immediate")
            .expect("First fetch failed");
        cache
            .all_ids()
            .now_or_never()
            .expect("Not Immediate")
            .expect("Second fetch failed");
    }

    #[test]
    fn can_be_seeded_with_a_cache() {
        let inner = MockTokenInfoFetching::new();
        let hardcoded = TokenBaseInfo {
            address: Address::from_low_u64_be(0),
            alias: "Foo".to_owned(),
            decimals: 42,
        };
        let cache = TokenInfoCache::with_cache(
            Arc::new(inner),
            hash_map! {
                TokenId::from(1) => hardcoded.clone()
            },
        );

        let info = cache
            .get_token_info(1.into())
            .now_or_never()
            .expect("First fetch not immediate")
            .expect("First fetch failed");
        assert_eq!(info, hardcoded);
    }

    #[test]
    fn cache_all_works() {
        fn token_ids() -> Vec<TokenId> {
            [0, 1, 2].iter().cloned().map(TokenId).collect()
        }

        let mut inner = MockTokenInfoFetching::new();
        inner
            .expect_all_ids()
            .times(1)
            .returning(|| immediate!(Ok(token_ids())));
        inner.expect_get_token_info().returning(|token_id| {
            if token_id.0 == 2 {
                immediate!(Err(anyhow!("")))
            } else {
                immediate!(Ok(TokenBaseInfo {
                    address: Address::from_low_u64_be(0),
                    alias: String::new(),
                    decimals: token_id.0 as u8,
                }))
            }
        });

        let cache = TokenInfoCache::new(Arc::new(inner));
        cache.cache_all().now_or_never().unwrap().unwrap();

        for token_id in token_ids() {
            let token_info = cache.get_token_info(token_id).now_or_never().unwrap();
            if token_id.0 == 2 {
                assert!(token_info.is_err());
            } else {
                let token_info = token_info.unwrap();
                assert_eq!(token_info.decimals, token_id.0 as u8);
            }
        }
    }

    #[test]
    fn get_token_infos_fetches_missing_infos() {
        let mut inner = MockTokenInfoFetching::new();
        inner
            .expect_get_token_info()
            .times(4)
            .returning(|token_id| {
                immediate!(Ok(TokenBaseInfo {
                    address: Address::from_low_u64_be(0),
                    alias: token_id.to_string(),
                    decimals: 1
                }))
            });

        let cache = TokenInfoCache::new(Arc::new(inner));
        let result = cache
            .get_token_infos(&[TokenId(0), TokenId(1)])
            .now_or_never()
            .unwrap()
            .unwrap();
        assert_eq!(result.len(), 2);
        assert_eq!(result.get(&TokenId(0)).unwrap().alias, "0");
        assert_eq!(result.get(&TokenId(1)).unwrap().alias, "1");

        let result = cache
            .get_token_infos(&[TokenId(1), TokenId(2), TokenId(3)])
            .now_or_never()
            .unwrap()
            .unwrap();
        assert_eq!(result.len(), 3);
        assert_eq!(result.get(&TokenId(1)).unwrap().alias, "1");
        assert_eq!(result.get(&TokenId(2)).unwrap().alias, "2");
        assert_eq!(result.get(&TokenId(3)).unwrap().alias, "3");
    }

    #[test]
    fn find_token_by_symbol_doesnt_query_if_in_cache() {
        let owl = TokenBaseInfo {
            address: Address::from_low_u64_be(0),
            alias: "OWL".to_owned(),
            decimals: 18,
        };

        let inner = MockTokenInfoFetching::new();
        let cache = TokenInfoCache::with_cache(
            Arc::new(inner),
            hash_map! {
                TokenId(0) => owl.clone(),
            },
        );

        assert_eq!(
            cache
                .find_token_by_symbol("OWL")
                .now_or_never()
                .unwrap()
                .unwrap(),
            Some((TokenId(0), owl)),
        );
    }

    #[test]
    fn find_token_by_symbol_updates_cache_for_missing_symbol() {
        let owl = TokenBaseInfo {
            address: Address::from_low_u64_be(0),
            alias: "OWL".to_owned(),
            decimals: 18,
        };

        let mut inner = MockTokenInfoFetching::new();
        inner
            .expect_all_ids()
            .returning(|| immediate!(Ok(vec![TokenId(0)])));
        inner
            .expect_get_token_info()
            .with(eq(TokenId(0)))
            .returning({
                let owl = owl.clone();
                move |_| immediate!(Ok(owl.clone()))
            });

        let cache = TokenInfoCache::new(Arc::new(inner));

        assert_eq!(
            cache
                .find_token_by_symbol("OWL")
                .now_or_never()
                .unwrap()
                .unwrap(),
            Some((TokenId(0), owl)),
        );
    }

    #[test]
    fn prefers_symbol_of_lower_token_ids() {
        // NOTE: The order in which entries get iterated with in a `HashMap` is
        // random, so use a large one with many many tokens so the chance of
        // the first one being having the lowest token ID is small.
        let cache = (0..1000).map(|id| {
            (
                TokenId(id),
                TokenBaseInfo {
                    address: Address::from_low_u64_be(0),
                    alias: "OWL".to_owned(),
                    decimals: 18,
                },
            )
        });

        let inner = MockTokenInfoFetching::new();
        let cache = TokenInfoCache::with_cache(Arc::new(inner), cache);

        let (id, _) = cache
            .find_token_by_symbol("OWL")
            .now_or_never()
            .unwrap()
            .unwrap()
            .unwrap(); // 🤣
        assert_eq!(id, TokenId(0));
    }

    #[test]
    fn fetches_tokens_with_lower_ids_when_searching_for_symbol() {
        let owl = TokenBaseInfo {
            address: Address::from_low_u64_be(0),
            alias: "OWL".to_owned(),
            decimals: 18,
        };

        let mut inner = MockTokenInfoFetching::new();
        inner
            .expect_get_token_info()
            .with(eq(TokenId(0)))
            .returning({
                let owl = owl.clone();
                move |_| immediate!(Ok(owl.clone()))
            });

        let cache = TokenInfoCache::with_cache(
            Arc::new(inner),
            hash_map! {
                TokenId(1) => owl.clone(),
            },
        );

        assert_eq!(
            cache
                .find_token_by_symbol("OWL")
                .now_or_never()
                .unwrap()
                .unwrap(),
            Some((TokenId(0), owl)),
        );
    }
}
