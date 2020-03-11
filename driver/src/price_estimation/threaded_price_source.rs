use super::{price_source::PriceSource, Token};
use crate::models::TokenId;
use anyhow::Result;
use std::collections::HashMap;
use std::sync::{
    mpsc::{self, RecvTimeoutError, Sender},
    Arc, Mutex,
};
use std::thread::{self, JoinHandle};
use std::time::Duration;

/// Implements `PriceSource` in a non blocking way by updating prices in a
/// thread and reusing previous results.
pub struct ThreadedPriceSource {
    // Shared between this struct and the thread. The background thread writes
    // the prices, the thread calling `get_prices` reads them.
    price_map: Arc<Mutex<HashMap<TokenId, u128>>>,
    // Allows the thread to notice when the owning struct is dropped.
    _channel_to_thread: Sender<()>,
}

impl ThreadedPriceSource {
    /// All token prices will be updated every `update_interval`. Prices for
    /// other tokens will not be returned in `get_prices`.
    ///
    /// The join handle represents the background thread. It can be used to
    /// verify that it exits when the created struct is dropped.
    #[allow(dead_code)]
    pub fn new<T: 'static + PriceSource + Send>(
        tokens: Vec<Token>,
        price_source: T,
        update_interval: Duration,
    ) -> (Self, JoinHandle<()>) {
        let price_map = Arc::new(Mutex::new(HashMap::new()));
        let (sender, receiver) = mpsc::channel::<()>();

        let price_map_ = price_map.clone();
        // vk: Clippy notes that the following loop and match can be written as
        // a `while let` loop but I find it clearer to make all cases explicit.
        #[allow(clippy::while_let_loop)]
        let join_handle = thread::spawn(move || loop {
            match receiver.recv_timeout(update_interval) {
                Ok(()) | Err(RecvTimeoutError::Timeout) => {
                    // Make sure we don't hold the mutex while this blocking call happens.
                    match price_source.get_prices(&tokens) {
                        Ok(prices) => price_map_.lock().unwrap().extend(prices),
                        Err(err) => log::warn!("price_source::get_prices failed: {}", err),
                    }
                }
                // The owning struct has been dropped.
                Err(RecvTimeoutError::Disconnected) => break,
            }
        });

        // Update the prices immediately.
        sender.send(()).unwrap();
        (
            Self {
                price_map,
                _channel_to_thread: sender,
            },
            join_handle,
        )
    }
}

impl PriceSource for ThreadedPriceSource {
    /// Non blocking.
    /// Infallible.
    fn get_prices(&self, _tokens: &[Token]) -> Result<HashMap<TokenId, u128>> {
        Ok(self.price_map.lock().unwrap().clone())
    }
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
        let mut ps = MockPriceSource::new();
        ps.expect_get_prices().returning(|_| Ok(HashMap::new()));
        let (tps, handle) =
            ThreadedPriceSource::new(TOKENS.to_vec(), ps, Duration::from_secs(std::u64::MAX));
        // If the thread didn't exit we would wait forever.
        join(tps, handle);
    }

    #[test]
    fn update_triggered_by_interval() {
        let mut price_source = MockPriceSource::new();
        let start_returning = Arc::new(atomic::AtomicBool::new(false));
        let start_returning_ = start_returning.clone();
        price_source.expect_get_prices().returning(move |_| {
            Ok(if start_returning_.load(ORDERING) {
                hash_map! {TOKENS[0].id => 1}
            } else {
                hash_map! {}
            })
        });

        let (tps, handle) =
            ThreadedPriceSource::new(TOKENS.to_vec(), price_source, Duration::from_millis(5));
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
