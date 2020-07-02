use super::{TokenBaseInfo, TokenId, TokenInfoFetching};

use anyhow::Result;
use async_std::sync::RwLock;
use futures::future::{BoxFuture, FutureExt};
use std::collections::HashMap;
use std::sync::Arc;

/**
 * Implementation of TokenInfoFetching that stores previously fetched information in an in-memory cache for fast retrieval.
 * TokenIds will always be fetched from the inner layer, as new tokens could be added at any time.
 */
struct TokenInfoCache {
    cache: RwLock<HashMap<TokenId, TokenBaseInfo>>,
    inner: Arc<dyn TokenInfoFetching>,
}

impl TokenInfoCache {
    #[allow(dead_code)]
    pub fn new(inner: Arc<dyn TokenInfoFetching>) -> Self {
        Self {
            cache: RwLock::new(HashMap::new()),
            inner,
        }
    }

    #[allow(dead_code)]
    pub fn with_cache(
        inner: Arc<dyn TokenInfoFetching>,
        cache: HashMap<TokenId, TokenBaseInfo>,
    ) -> Self {
        Self {
            inner,
            cache: RwLock::new(cache),
        }
    }
}

impl TokenInfoFetching for TokenInfoCache {
    fn get_token_info<'a>(&'a self, id: TokenId) -> BoxFuture<'a, Result<TokenBaseInfo>> {
        async move {
            if let Some(info) = self.cache.read().await.get(&id).cloned() {
                return Ok(info);
            }
            let info = self.inner.get_token_info(id).await;
            if let Ok(info) = info.as_ref() {
                self.cache.write().await.insert(id, info.clone());
            }
            info
        }
        .boxed()
    }

    fn all_ids<'a>(&'a self) -> BoxFuture<'a, Result<Vec<TokenId>>> {
        self.inner.all_ids()
    }
}

#[cfg(test)]
mod tests {
    use super::super::MockTokenInfoFetching;
    use super::*;
    use anyhow::anyhow;

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
}
