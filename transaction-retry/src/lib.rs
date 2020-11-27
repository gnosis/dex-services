#![deny(missing_docs)]

//! Keep sending a transaction with updated gas price.

mod first_match;
pub mod gas_price_increase;

use first_match::FirstMatchOrLast;
use futures::{
    future::FutureExt as _,
    stream::{Stream, StreamExt as _},
};
use std::future::Future;

/// Implementation agnostic abstraction over sending a specific ethereum transaction.
#[async_trait::async_trait]
#[cfg_attr(test, mockall::automock(type Output = bool;))]
pub trait TransactionSending: Send {
    /// The result of sending the transaction.
    type Output: TransactionResult;
    /// Send the transaction.
    async fn send(&self, gas_price: f64) -> Self::Output;
}

/// Trait that the result of sent transactions must implement.
pub trait TransactionResult {
    /// Was the transaction mined or did it error before? This happens when a transaction was
    /// replaced with a higher gas price.
    fn was_mined(&self) -> bool;
}

// For mocking we picked the associated type to be bool so it must implement this trait.
#[cfg(test)]
impl TransactionResult for bool {
    fn was_mined(&self) -> bool {
        *self
    }
}

/// The result of retry function.
pub enum RetryResult<TransactionResult, CancellationResult> {
    /// The transaction was submitted.
    Submitted(TransactionResult),
    /// The transaction was cancelled.
    Cancelled(CancellationResult),
}

/// Send a transaction whenever the next gas price is ready.
/// When cancel_after is ready the transaction will be cancelled by sending a noop transaction
/// at a higher gas price.
/// Returns None if no transaction has been submitted.
/// Panics if the stream ends.
pub async fn retry<TransactionSender, CancellationSender>(
    transaction_sender: TransactionSender,
    cancel_after: impl Future<Output = CancellationSender>,
    gas_price_stream: impl Stream<Item = f64>,
) -> Option<RetryResult<TransactionSender::Output, CancellationSender::Output>>
where
    TransactionSender: TransactionSending,
    CancellationSender: TransactionSending,
{
    // make useable in `select!`
    let gas_price_stream = gas_price_stream.fuse();
    let cancel_after = cancel_after.fuse();
    futures::pin_mut!(cancel_after);
    futures::pin_mut!(gas_price_stream);

    // This struct keeps track of all the solution and cancellation futures. If we get a
    // "nonce already used error" we continue running the other futures. We need to handle this
    // case because we do not know which transactions will complete or fail or in which order we
    // observe completion.
    let mut first_match = FirstMatchOrLast::new(|result: &RetryResult<_, _>| result.was_mined());
    let mut last_used_gas_price = 0.0;
    let cancellation_sender = loop {
        // Use select_biased over select because it makes tests deterministic. for real use doesn't
        // matter because the futures will almost never become ready at the same time.
        futures::select_biased! {
            result = first_match => return Some(result),
            cancellation_sender = cancel_after => break cancellation_sender,
            gas_price = gas_price_stream.next() => {
                let gas_price = gas_price.expect("stream never ends");
                last_used_gas_price = gas_price;
                first_match.add(transaction_sender.send(gas_price).map(RetryResult::Submitted).boxed());
            }
        }
    };
    // Need to do this so that compiler doesn't complain that first_match gets dropped first.
    let (cancellation_sender, first_match) = (cancellation_sender, first_match);

    let never_submitted_solution = last_used_gas_price == 0.0;
    if never_submitted_solution {
        return None;
    }
    let gas_price = gas_price_increase::minimum_increase(last_used_gas_price);
    let cancellation = cancellation_sender
        .send(gas_price)
        .map(RetryResult::Cancelled)
        .boxed();
    first_match.add(cancellation);
    Some(first_match.await)
}

impl<T0: TransactionResult, T1: TransactionResult> TransactionResult for RetryResult<T0, T1> {
    fn was_mined(&self) -> bool {
        match self {
            Self::Submitted(result) => result.was_mined(),
            Self::Cancelled(result) => result.was_mined(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::{future, stream};

    #[test]
    fn nonce_error_ignored() {
        let mut transaction_sender = MockTransactionSending::new();
        let cancel_after = future::pending::<MockTransactionSending>().boxed();
        let (sender, receiver) = futures::channel::oneshot::channel();

        transaction_sender.expect_send().times(1).return_once(|_| {
            async move {
                receiver.await.unwrap();
                true
            }
            .boxed()
        });
        transaction_sender.expect_send().times(1).return_once(|_| {
            sender.send(()).unwrap();
            future::ready(false).boxed()
        });

        let result = retry(transaction_sender, cancel_after, stream::repeat(1.0));
        let result = result.now_or_never().unwrap();
        assert!(matches!(result, Some(RetryResult::Submitted(true))));
    }

    #[test]
    fn submission_completes_during_cancellation() {
        let (cancel_sender, cancel_receiver) = futures::channel::oneshot::channel();
        let (submit_sender, submit_receiver) = futures::channel::oneshot::channel();
        let mut transaction_sender = MockTransactionSending::new();
        let mut cancellation_sender = MockTransactionSending::new();

        transaction_sender.expect_send().times(1).return_once(|_| {
            async move {
                cancel_sender.send(()).unwrap();
                submit_receiver.await.unwrap();
                true
            }
            .boxed()
        });
        cancellation_sender
            .expect_send()
            .times(1)
            .return_once(|_| future::pending().boxed());
        let cancel_after = async move {
            cancel_receiver.await.unwrap();
            submit_sender.send(()).unwrap();
            cancellation_sender
        }
        .boxed();

        let result = retry(transaction_sender, cancel_after, stream::repeat(1.0));
        let result = result.now_or_never().unwrap();
        assert!(matches!(result, Some(RetryResult::Submitted(true))));
    }

    #[test]
    fn cancellation_completes() {
        let (cancel_sender, cancel_receiver) = futures::channel::oneshot::channel();
        let mut transaction_sender = MockTransactionSending::new();
        let mut cancellation_sender = MockTransactionSending::new();

        transaction_sender.expect_send().times(1).return_once(|_| {
            cancel_sender.send(()).unwrap();
            future::pending().boxed()
        });
        cancellation_sender
            .expect_send()
            .times(1)
            .returning(|_| future::ready(true).boxed());
        let cancel_after = async move {
            cancel_receiver.await.unwrap();
            cancellation_sender
        }
        .boxed();

        let result = retry(transaction_sender, cancel_after, stream::repeat(1.0));
        let result = result.now_or_never().unwrap();
        assert!(matches!(result, Some(RetryResult::Cancelled(true))));
    }
}
