use super::price_source::PriceSource;
use crate::{models::TokenId, util::FutureWaitExt as _};
use anyhow::Result;
use futures::{
    future::{BoxFuture, FutureExt as _},
    lock::Mutex,
};
use std::collections::HashMap;
use std::panic::{self, AssertUnwindSafe};
#[cfg(test)]
use std::sync::mpsc::Receiver;
use std::sync::{
    mpsc::{self, RecvTimeoutError, Sender},
    Arc,
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
    // Mutex is not needed because we don't access the sender at all but we need
    // this struct to be sync and this makes the compiler happy.
    _channel_to_thread: Mutex<Sender<()>>,

    // To make testing easier we use another channel to which the thread sends
    // a message whenever it completes one update loop.
    #[cfg(test)]
    test_receiver: Mutex<Receiver<()>>,
}

impl ThreadedPriceSource {
    /// All token prices will be updated every `update_interval`. Prices for
    /// other tokens will not be returned in `get_prices`.
    ///
    /// The join handle represents the background thread. It can be used to
    /// verify that it exits when the created struct is dropped.
    pub fn new<T: 'static + PriceSource + Send>(
        tokens: Vec<TokenId>,
        price_source: T,
        update_interval: Duration,
    ) -> (Self, JoinHandle<()>) {
        let price_map = Arc::new(Mutex::new(HashMap::new()));
        let (sender, receiver) = mpsc::channel::<()>();

        #[cfg(test)]
        let (test_sender, test_receiver) = mpsc::channel::<()>();

        let join_handle = thread::spawn({
            let price_map = price_map.clone();
            let price_map_catch = price_map.clone();

            // vk: Clippy notes that the following loop and match can be written
            // as a `while let` loop but I find it clearer to make all cases
            // explicit.
            move || {
                #[allow(clippy::while_let_loop)]
                let result = panic::catch_unwind(AssertUnwindSafe(move || loop {
                    match receiver.recv_timeout(update_interval) {
                        Ok(()) | Err(RecvTimeoutError::Timeout) => {
                            // Make sure we don't hold the mutex while this blocking call happens.
                            match price_source.get_prices(&tokens).wait() {
                                Ok(prices) => price_map.lock().wait().extend(prices),
                                Err(err) => log::warn!("price_source::get_prices failed: {}", err),
                            }
                        }
                        // The owning struct has been dropped.
                        Err(RecvTimeoutError::Disconnected) => break,
                    }

                    #[cfg(test)]
                    let _ = test_sender.send(());
                }));

                if let Err(err) = result {
                    log::error!("threaded price source thread panicked");
                    // NOTE: Ensure that we try to clear the price to not report
                    // outdated ones.
                    price_map_catch.lock().wait().clear();
                    panic::resume_unwind(err);
                }
            }
        });

        // Update the prices immediately.
        sender.send(()).unwrap();
        (
            Self {
                price_map,
                _channel_to_thread: Mutex::new(sender),
                #[cfg(test)]
                test_receiver: Mutex::new(test_receiver),
            },
            join_handle,
        )
    }
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
    use std::sync::atomic;
    use std::time::Instant;

    const ORDERING: atomic::Ordering = atomic::Ordering::SeqCst;
    const THREAD_TIMEOUT: Duration = Duration::from_secs(1);

    lazy_static::lazy_static! {
        static ref TOKENS: [TokenId; 3] = [
            TokenId(0),
            TokenId(1),
            TokenId(2),
        ];
    }

    /// Drops tps and joins the handle which ensures that the thread exits and
    /// hasn't panicked. This is important because the mockall expectation
    /// panics happen in the thread which would otherwise let the tests pass
    /// when an expectation is violated.
    fn join(tps: ThreadedPriceSource, handle: JoinHandle<()>) {
        fn extract_test_receiver(tps: ThreadedPriceSource) -> Receiver<()> {
            tps.test_receiver.into_inner()
        }
        let test_receiver = extract_test_receiver(tps);
        let deadline = Instant::now() + THREAD_TIMEOUT;
        loop {
            match test_receiver.recv_timeout(THREAD_TIMEOUT) {
                // The sender side of `test_receiver` has been dropped which
                // only happens when the thread exits.
                Err(RecvTimeoutError::Disconnected) => break,
                // There is still a queued messages on the channel.
                Ok(_) if Instant::now() <= deadline => continue,
                Ok(_) | Err(RecvTimeoutError::Timeout) =>
                    panic!("Background thread of ThreadedPriceSource did not exit in time after owner was dropped."),
            }
        }
        // Propagate any panic.
        // Does not block because we know the thread has exited.
        handle.join().unwrap();
    }

    fn wait_for_thread_to_loop(tps: &ThreadedPriceSource) {
        // Clear previous messages.
        let deadline = Instant::now() + THREAD_TIMEOUT;
        for _ in tps.test_receiver.lock().now_or_never().unwrap().try_iter() {
            if Instant::now() >= deadline {
                panic!("Background thread of ThreadedPriceSource ran too many loops.");
            }
        }
        // Wait for one new message.
        tps.test_receiver
            .lock()
            .now_or_never()
            .unwrap()
            .recv_timeout(THREAD_TIMEOUT)
            .unwrap();
    }

    #[test]
    fn thread_exits_when_owner_is_dropped() {
        let mut ps = MockPriceSource::new();
        ps.expect_get_prices()
            .returning(|_| async { Ok(HashMap::new()) }.boxed());
        let (tps, handle) =
            ThreadedPriceSource::new(TOKENS.to_vec(), ps, Duration::from_secs(std::u64::MAX));
        join(tps, handle);
    }

    #[test]
    fn update_triggered_by_interval() {
        let mut price_source = MockPriceSource::new();
        let start_returning = Arc::new(atomic::AtomicBool::new(false));
        price_source.expect_get_prices().returning({
            let start_returning = start_returning.clone();
            move |_| {
                let start_returning_ = start_returning.clone();
                async move {
                    Ok(if start_returning_.load(ORDERING) {
                        hash_map! {TOKENS[0] => 1}
                    } else {
                        hash_map! {}
                    })
                }
                .boxed()
            }
        });

        let (tps, handle) =
            ThreadedPriceSource::new(TOKENS.to_vec(), price_source, Duration::from_millis(1));
        assert_eq!(
            tps.get_prices(&TOKENS[..]).now_or_never().unwrap().unwrap(),
            hash_map! {}
        );
        wait_for_thread_to_loop(&tps);
        start_returning.store(true, ORDERING);
        wait_for_thread_to_loop(&tps);
        assert_eq!(
            tps.get_prices(&TOKENS[..]).now_or_never().unwrap().unwrap(),
            hash_map! {TOKENS[0] => 1}
        );
        join(tps, handle);
    }
}
