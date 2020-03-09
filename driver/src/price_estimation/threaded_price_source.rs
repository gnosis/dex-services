use super::manually_updated_price_source::ManuallyUpdatedPriceSource;
use super::{price_source::PriceSource, Token};
use crate::models::TokenId;
use anyhow::Result;
use std::collections::HashMap;
use std::sync::{
    mpsc::{self, RecvTimeoutError, Sender},
    Arc, Mutex,
};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

/// Implements `PriceSource` in a non blocking way by updating prices in a
/// thread and reusing previous results.
pub struct ThreadedPriceSource {
    // Shared between this struct and the thread. The background thread writes
    // the prices, the thread calling `get_prices` reads them.
    non_blocking_price_source: Arc<Mutex<ManuallyUpdatedPriceSource>>,
    // Channel over which the thread can be told to update the prices.
    channel_to_thread: Sender<ThreadMessage>,
}

impl ThreadedPriceSource {
    /// The join handle represents the background thread. It can be used to
    /// verify that it exits when the created struct is dropped.
    #[allow(dead_code)]
    pub fn new<T: 'static + PriceSource + Send>(
        price_source: T,
        update_interval: Duration,
    ) -> (Self, JoinHandle<()>) {
        let non_blocking_price_source = Arc::new(Mutex::new(ManuallyUpdatedPriceSource::new()));
        let (sender, receiver) = mpsc::channel::<ThreadMessage>();

        let nbps = non_blocking_price_source.clone();
        // vk: Clippy notes that the following loop and match can be written as
        // a `while let` loop but I find it clearer to make all cases explicit.
        #[allow(clippy::while_let_loop)]
        let join_handle = thread::spawn(move || loop {
            match receiver.recv_timeout(update_interval) {
                Ok(ThreadMessage::UpdateImmediately) | Err(RecvTimeoutError::Timeout) => {
                    let now = Instant::now();
                    let cutoff = now.checked_sub(update_interval).unwrap_or(now);
                    let tokens = nbps.lock().unwrap().tokens_that_need_updating(cutoff);
                    if tokens.is_empty() {
                        continue;
                    }
                    // Make sure we don't hold the mutex while this blocking call happens.
                    match price_source.get_prices(&tokens) {
                        Ok(prices) => nbps.lock().unwrap().update_tokens(&tokens, &prices, now),
                        Err(err) => log::warn!("price_source::get_prices failed: {}", err),
                    }
                }
                // The owning struct has been dropped.
                Err(RecvTimeoutError::Disconnected) => break,
            }
        });

        (
            Self {
                non_blocking_price_source,
                channel_to_thread: sender,
            },
            join_handle,
        )
    }
}

impl PriceSource for ThreadedPriceSource {
    /// Non blocking.
    /// Infallible.
    fn get_prices(&self, tokens: &[Token]) -> Result<HashMap<TokenId, u128>> {
        let mut nbps = self.non_blocking_price_source.lock().unwrap();
        nbps.track_tokens(tokens);
        // At this point tokens that were not being tracked already have been
        // added but they do not yet have prices. This is expected because this
        // function is supposed to be non blocking.
        let result = nbps.get_prices(tokens);
        std::mem::drop(nbps);
        // We do tell the thread to immediately update the prices instead of
        // waiting a full update interval so that the prices are hopefully
        // available the next `get_prices` is called.
        self.channel_to_thread
            .send(ThreadMessage::UpdateImmediately)
            .unwrap();
        result
    }
}

enum ThreadMessage {
    UpdateImmediately,
}

#[cfg(test)]
mod tests {
    use super::super::price_source::MockPriceSource;
    use super::*;
    use std::sync::atomic;

    const ORDERING: atomic::Ordering = atomic::Ordering::SeqCst;

    lazy_static::lazy_static! {
        static ref TOKENS: [Token; 3] = [
            Token::new(0, "0", 0),
            Token::new(1, "1", 1),
            Token::new(2, "2", 2),
        ];
    }

    /// Drops tps and joins the handle which ensures that the thread exits and
    /// hasn't panicked. This is important because the mockall expectation
    /// panics happen in the thread which would otherwise let the tests pass
    /// when an expectation is violated.
    fn join(tps: ThreadedPriceSource, handle: JoinHandle<()>) {
        std::mem::drop(tps);
        handle.join().unwrap();
    }

    #[test]
    fn thread_exits_when_owner_is_dropped() {
        let (tps, handle) =
            ThreadedPriceSource::new(MockPriceSource::new(), Duration::from_secs(std::u64::MAX));
        // If the thread didn't exit we would wait forever.
        join(tps, handle);
    }

    #[test]
    fn update_triggered_by_get_prices() {
        let mut ps = MockPriceSource::new();
        let call_count = Arc::new(atomic::AtomicU64::new(0));
        let call_count_ = call_count.clone();
        ps.expect_get_prices()
            .returning(move |_| match call_count_.fetch_add(1, ORDERING) {
                0 => Ok(hash_map! {TOKENS[0].id => 0}),
                1 => Ok(hash_map! {TOKENS[0].id => 1, TOKENS[1].id => 2}),
                _ => Ok(hash_map! {}),
            });

        let (tps, handle) = ThreadedPriceSource::new(ps, Duration::from_secs(std::u64::MAX));
        assert_eq!(call_count.load(ORDERING), 0);
        assert!(tps.get_prices(&TOKENS[..]).unwrap().is_empty());

        // We have to sleep to give the thread time to work.
        thread::sleep(Duration::from_millis(5));
        assert_eq!(call_count.load(ORDERING), 1);
        assert_eq!(
            tps.get_prices(&TOKENS[..]).unwrap(),
            hash_map! {TOKENS[0].id => 0}
        );

        thread::sleep(Duration::from_millis(5));
        assert_eq!(call_count.load(ORDERING), 2);
        assert_eq!(
            tps.get_prices(&TOKENS[..]).unwrap(),
            hash_map! {TOKENS[0].id => 1, TOKENS[1].id => 2}
        );

        thread::sleep(Duration::from_millis(5));
        assert_eq!(call_count.load(ORDERING), 3);
        assert_eq!(
            tps.get_prices(&TOKENS[..]).unwrap(),
            hash_map! {TOKENS[0].id => 1, TOKENS[1].id => 2}
        );

        thread::sleep(Duration::from_millis(5));
        assert_eq!(call_count.load(ORDERING), 4);
        assert_eq!(
            tps.get_prices(&TOKENS[..]).unwrap(),
            hash_map! {TOKENS[0].id => 1, TOKENS[1].id => 2}
        );
        join(tps, handle);
    }

    #[test]
    fn update_triggered_by_interval() {
        let mut ps = MockPriceSource::new();
        let start_returning = Arc::new(atomic::AtomicBool::new(false));
        let start_returning_ = start_returning.clone();
        ps.expect_get_prices().returning(move |_| {
            Ok(if start_returning_.load(ORDERING) {
                hash_map! {TOKENS[0].id => 1}
            } else {
                hash_map! {}
            })
        });

        let (tps, handle) = ThreadedPriceSource::new(ps, Duration::from_millis(5));
        assert_eq!(tps.get_prices(&TOKENS[..]).unwrap(), hash_map! {});
        thread::sleep(Duration::from_millis(10));
        start_returning.store(true, ORDERING);
        thread::sleep(Duration::from_millis(10));
        assert_eq!(
            tps.get_prices(&TOKENS[..]).unwrap(),
            hash_map! {TOKENS[0].id => 1}
        );
        join(tps, handle);
    }
}
