use super::price_source::PriceSource;
use crate::models::TokenId;
use crate::token_info::TokenInfoFetching;
use anyhow::Result;
use async_std::{
    sync::Mutex,
    task::{self, JoinHandle},
};
use futures::future::{BoxFuture, FutureExt as _};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

/// Implements a `PriceSource` that is always immediately ready by updating itself in a background
/// task and using the most recent update.
pub struct ThreadedPriceSource {
    // Shared between this struct and the task. The task writes the prices and `get_prices` reads
    // them.
    price_map: Arc<Mutex<HashMap<TokenId, u128>>>,
}

impl ThreadedPriceSource {
    /// All token prices will be updated every `update_interval`. Prices for
    /// other tokens will not be returned in `get_prices`.
    ///
    /// The join handle represents the task. It can be used to verify that it exits when the struct
    /// is dropped.
    pub fn new<T: 'static + PriceSource + Send + Sync>(
        token_info_fetcher: Arc<dyn TokenInfoFetching>,
        price_source: T,
        update_interval: Duration,
    ) -> (Self, JoinHandle<()>) {
        let price_map = Arc::new(Mutex::new(HashMap::new()));
        let join_handle = task::spawn({
            let price_map = Arc::downgrade(&price_map);
            async move {
                while let Some(price_map) = price_map.upgrade() {
                    match update_prices(&price_source, token_info_fetcher.as_ref()).await {
                        Ok(prices) => price_map.lock().await.extend(prices),
                        Err(err) => log::warn!("price_source::get_prices failed: {}", err),
                    }
                    task::sleep(update_interval).await;
                }
            }
        });
        (Self { price_map }, join_handle)
    }
}

async fn update_prices<T: PriceSource>(
    price_source: &T,
    token_info_fetching: &dyn TokenInfoFetching,
) -> Result<HashMap<TokenId, u128>> {
    let tokens = token_info_fetching.all_ids().await?;
    price_source.get_prices(&tokens).await
}

impl PriceSource for ThreadedPriceSource {
    fn get_prices<'a>(
        &'a self,
        tokens: &'a [TokenId],
    ) -> BoxFuture<'a, Result<HashMap<TokenId, u128>>> {
        async move {
            let price_map = self.price_map.lock().await;
            Ok(tokens
                .iter()
                .filter_map(|token| Some((*token, *price_map.get(&token)?)))
                .collect())
        }
        .boxed()
    }
}

#[cfg(test)]
mod tests {
    use super::super::price_source::MockPriceSource;
    use super::*;
    use crate::token_info::MockTokenInfoFetching;
    use crate::util::FutureWaitExt;
    use futures::future::{self, Either};
    use std::sync::atomic;
    use std::time::Instant;

    const ORDERING: atomic::Ordering = atomic::Ordering::SeqCst;
    const THREAD_TIMEOUT: Duration = Duration::from_secs(1);
    const UPDATE_INTERVAL: Duration = Duration::from_millis(1);
    const TOKENS: [TokenId; 1] = [TokenId(0)];

    /// Drops tps and joins the handle which ensures that the thread exits and
    /// hasn't panicked. This is important because the mockall expectation
    /// panics happen in the thread which would otherwise let the tests pass
    /// when an expectation is violated.
    async fn join(tps: ThreadedPriceSource, handle: JoinHandle<()>) {
        std::mem::drop(tps);
        let timeout = task::sleep(THREAD_TIMEOUT);
        futures::pin_mut!(timeout);
        if let Either::Right(_) = future::select(handle, timeout).await {
            panic!("Background task of ThreadedPriceSource did not exit in time after owner was dropped.");
        }
    }

    fn wait_for_condition(mut condition: impl FnMut() -> bool, deadline: Instant) {
        while !condition() {
            assert!(Instant::now() <= deadline, "condition not true in time");
            std::thread::sleep(UPDATE_INTERVAL);
        }
    }

    #[test]
    fn thread_exits_when_owner_is_dropped() {
        let mut ps = MockPriceSource::new();
        ps.expect_get_prices()
            .returning(|_| immediate!(Ok(HashMap::new())));

        let mut token_info_fetcher = MockTokenInfoFetching::new();
        token_info_fetcher
            .expect_all_ids()
            .returning(|| immediate!(Ok(TOKENS.to_vec())));

        let (tps, handle) =
            ThreadedPriceSource::new(Arc::new(token_info_fetcher), ps, UPDATE_INTERVAL);
        join(tps, handle).wait();
    }

    #[test]
    fn update_triggered_by_interval() {
        let mut price_source = MockPriceSource::new();
        let price = Arc::new(atomic::AtomicU8::new(0));
        price_source.expect_get_prices().returning({
            let price = price.clone();
            move |_| {
                let price_ = price.clone();
                async move { Ok(hash_map! { TOKENS[0] => price_.load(ORDERING) as u128 }) }.boxed()
            }
        });

        let mut token_info_fetcher = MockTokenInfoFetching::new();
        token_info_fetcher
            .expect_all_ids()
            .returning(|| immediate!(Ok(TOKENS.to_vec())));

        let (tps, handle) =
            ThreadedPriceSource::new(Arc::new(token_info_fetcher), price_source, UPDATE_INTERVAL);
        let get_prices = || tps.get_prices(&TOKENS[..]).wait().unwrap();
        price.store(1, ORDERING);
        let condition = || get_prices().get(&TOKENS[0]) == Some(&1);
        wait_for_condition(condition, Instant::now() + THREAD_TIMEOUT);
        price.store(2, ORDERING);
        let condition = || get_prices().get(&TOKENS[0]) == Some(&2);
        wait_for_condition(condition, Instant::now() + THREAD_TIMEOUT);
        join(tps, handle).wait();
    }
}
