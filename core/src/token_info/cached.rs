use super::{TokenBaseInfo, TokenId, TokenInfoFetching};

use anyhow::{anyhow, Context as _, Result};
use async_std::sync::{Mutex, RwLock};
use futures::{
    future::{BoxFuture, FutureExt, Shared},
    stream::{self, StreamExt as _},
};
use std::collections::HashMap;
use std::sync::Arc;

/// Implementation of TokenInfoFetching that stores previously fetched information in an in-memory cache for fast retrieval.
/// TokenIds will always be fetched from the inner layer, as new tokens could be added at any time.
pub struct TokenInfoCache {
    cache: Arc<Cache>,
    inner: Arc<dyn TokenInfoFetching>,
}

/// The cache used for the token info.
#[derive(Debug, Default)]
struct Cache {
    infos: RwLock<HashMap<TokenId, TokenBaseInfo>>,
    pending: Mutex<HashMap<TokenId, PendingTokenInfo>>,
}

/// Type alias for shared token info fetching futures.
type PendingTokenInfo = Shared<BoxFuture<'static, Result<(), String>>>;

impl TokenInfoCache {
    pub fn new(inner: Arc<dyn TokenInfoFetching>) -> Self {
        Self {
            cache: Default::default(),
            inner,
        }
    }

    #[allow(dead_code)]
    pub fn with_cache(
        inner: Arc<dyn TokenInfoFetching>,
        cache: HashMap<TokenId, TokenBaseInfo>,
    ) -> Self {
        Self {
            cache: Arc::new(Cache {
                infos: RwLock::new(cache),
                pending: Default::default(),
            }),
            inner,
        }
    }

    /// Attempt to retrieve and cache all token info that is not already cached.
    /// Fails if `all_ids` fails. Does not fail if individual token infos fail.
    pub async fn cache_all(&self, number_of_parallel_requests: usize) -> Result<()> {
        stream::iter(self.all_ids().await.context("failed to get all ids")?)
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

    /// Gets the cached token information if available. Returns `None` if the
    /// token info is not yet cached.
    async fn get_cached_token_info(&self, id: TokenId) -> Option<TokenBaseInfo> {
        self.cache.infos.read().await.get(&id).cloned()
    }

    /// Creates a shared future for retrieiving token info.
    ///
    /// These are used to guarantee that concurrent fibers fetching token info
    /// don't "double fetch".
    async fn fetch_and_cache_token_info(&self, id: TokenId) -> Result<()> {
        // NOTE: Because `Shared` futures require the output type to be `Clone`,
        // and `anyhow::Error: !Clone` we cannot propagate the error directly,
        // so we only propagate the error message and log the full error stack.
        let fetch_info = {
            let mut pending = self.cache.pending.lock().await;
            pending
                .entry(id)
                .or_insert_with(|| {
                    let cache = self.cache.clone();
                    let inner = self.inner.clone();

                    async move {
                        let result = match inner.get_token_info(id).await {
                            Ok(info) => {
                                cache.infos.write().await.insert(id, info);
                                Ok(())
                            }
                            Err(err) => {
                                log::debug!(
                                    "failed to fetch and cache info for token {}: {:?}",
                                    id,
                                    err
                                );
                                Err(err.to_string())
                            }
                        };

                        cache.pending.lock().await.remove(&id);
                        result
                    }
                    .boxed()
                    .shared()
                })
                .clone()
        };

        fetch_info.await.map_err(|message| {
            anyhow!(
                "error fetching and caching info for token {}: {}",
                id,
                message,
            )
        })?;

        Ok(())
    }
}

impl TokenInfoFetching for TokenInfoCache {
    fn get_token_info<'a>(&'a self, id: TokenId) -> BoxFuture<'a, Result<TokenBaseInfo>> {
        async move {
            if let Some(info) = self.get_cached_token_info(id).await {
                return Ok(info);
            }

            self.fetch_and_cache_token_info(id).await?;
            Ok(self
                .get_cached_token_info(id)
                .await
                .expect("missing token info after succesfully fetching and caching"))
        }
        .boxed()
    }

    fn all_ids<'a>(&'a self) -> BoxFuture<'a, Result<Vec<TokenId>>> {
        self.inner.all_ids()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{token_info::MockTokenInfoFetching, util::FutureWaitExt as _};
    use anyhow::anyhow;
    use futures::future;

    #[test]
    fn calls_inner_once_per_token_id_on_success() {
        let mut inner = MockTokenInfoFetching::new();

        inner.expect_get_token_info().times(1).returning(|_| {
            immediate!(Ok(TokenBaseInfo {
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
                    alias: String::new(),
                    decimals: token_id.0 as u8,
                }))
            }
        });

        let cache = TokenInfoCache::new(Arc::new(inner));
        cache.cache_all(2).now_or_never().unwrap().unwrap();

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
    fn token_infos_fetched_once() {
        let mut inner = MockTokenInfoFetching::new();
        // NOTE: Use `return_once` to ensure this test panics if there is
        // more than one request.
        inner.expect_get_token_info().return_once(|_| {
            immediate!(Ok(TokenBaseInfo {
                alias: "FOO".to_string(),
                decimals: 42,
            }))
        });

        let cache = TokenInfoCache::new(Arc::new(inner));

        let fetch1 = cache.get_token_info(0.into());
        let fetch2 = cache.get_token_info(0.into());

        let (info1, info2) = future::join(fetch1, fetch2).wait();
        assert_eq!(info1.unwrap(), info2.unwrap());
    }
}
