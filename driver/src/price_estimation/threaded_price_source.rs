use super::{price_source::PriceSource, Token};
use crate::models::TokenId;
use anyhow::Result;
use std::collections::HashMap;
#[cfg(test)]
use std::sync::mpsc::Receiver;
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

    // To make testing easier we use another channel to which the thread sends
    // a message whenever it completes one update loop.
    #[cfg(test)]
    test_receiver: Receiver<()>,
}

impl ThreadedPriceSource {
    /// All token prices will be updated every `update_interval`. Prices for
    /// other tokens will not be returned in `get_prices`.
    ///
    /// The join handle represents the background thread. It can be used to
    /// verify that it exits when the created struct is dropped.
    pub fn new<T: 'static + PriceSource + Send>(
        tokens: Vec<Token>,
        price_source: T,
        update_interval: Duration,
    ) -> (Self, JoinHandle<()>) {
        let price_map = Arc::new(Mutex::new(HashMap::new()));
        let (sender, receiver) = mpsc::channel::<()>();

        #[cfg(test)]
        let (test_sender, test_receiver) = mpsc::channel::<()>();

        let join_handle = thread::spawn({
            let price_map = price_map.clone();

            // vk: Clippy notes that the following loop and match can be written
            // as a `while let` loop but I find it clearer to make all cases
            // explicit.
            #[allow(clippy::while_let_loop)]
            move || loop {
                match receiver.recv_timeout(update_interval) {
                    Ok(()) | Err(RecvTimeoutError::Timeout) => {
                        // Make sure we don't hold the mutex while this blocking call happens.
                        match price_source.get_prices(&tokens) {
                            Ok(prices) => price_map
                                .lock()
                                .expect("mutex should never be poisoned")
                                .extend(prices),
                            Err(err) => log::warn!("price_source::get_prices failed: {}", err),
                        }
                    }
                    // The owning struct has been dropped.
                    Err(RecvTimeoutError::Disconnected) => break,
                }

                #[cfg(test)]
                let _ = test_sender.send(());
            }
        });

        // Update the prices immediately.
        sender.send(()).unwrap();
        (
            Self {
                price_map,
                _channel_to_thread: sender,
                #[cfg(test)]
                test_receiver,
            },
            join_handle,
        )
    }
}

impl PriceSource for ThreadedPriceSource {
    fn get_prices(&self, _tokens: &[Token]) -> Result<HashMap<TokenId, u128>> {
        // NOTE: Return all the prices as they will get filtered by the caller
        //   anyway.

        Ok(self
            .price_map
            .lock()
            .expect("mutex should never be poisoned")
            .clone())
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
        fn extract_test_receiver(tps: ThreadedPriceSource) -> Receiver<()> {
            tps.test_receiver
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
        for _ in tps.test_receiver.try_iter() {
            if Instant::now() >= deadline {
                panic!("Background thread of ThreadedPriceSource ran too many loops.");
            }
        }
        // Wait for one new message.
        tps.test_receiver.recv_timeout(THREAD_TIMEOUT).unwrap();
    }

    #[test]
    fn thread_exits_when_owner_is_dropped() {
        let mut ps = MockPriceSource::new();
        ps.expect_get_prices().returning(|_| Ok(HashMap::new()));
        let (tps, handle) =
            ThreadedPriceSource::new(TOKENS.to_vec(), ps, Duration::from_secs(std::u64::MAX));
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
            ThreadedPriceSource::new(TOKENS.to_vec(), price_source, Duration::from_millis(1));
        assert_eq!(tps.get_prices(&TOKENS[..]).unwrap(), hash_map! {});
        wait_for_thread_to_loop(&tps);
        start_returning.store(true, ORDERING);
        wait_for_thread_to_loop(&tps);
        assert_eq!(
            tps.get_prices(&TOKENS[..]).unwrap(),
            hash_map! {TOKENS[0].id => 1}
        );
        join(tps, handle);
    }
}
