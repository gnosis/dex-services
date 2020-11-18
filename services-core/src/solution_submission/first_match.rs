use futures::{
    future::FusedFuture,
    stream::{futures_unordered::FuturesUnordered, StreamExt as _},
};
use std::{future::Future, pin::Pin, task::Context, task::Poll};

/// Resolves to the first result of a set of inner futures that matches a filter.
/// Futures can be dynamically added to the inner future.
/// Stays pending until there is at least one future.
/// If the last inner future is ready its result is returned even if it doesn't match the filter.
pub struct FirstMatchOrLast<Fut, Filter> {
    futures: FuturesUnordered<Fut>,
    filter: Filter,
    is_terminated: bool,
}

impl<Fut, Filter> FirstMatchOrLast<Fut, Filter> {
    pub fn new(filter: Filter) -> Self {
        Self {
            filter,
            futures: FuturesUnordered::new(),
            is_terminated: false,
        }
    }

    pub fn add(&self, future: Fut) {
        self.futures.push(future);
    }
}

impl<Fut, Filter> Future for FirstMatchOrLast<Fut, Filter>
where
    Fut: Future,
    Filter: FnMut(&Fut::Output) -> bool + Unpin,
{
    type Output = Fut::Output;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        // The stream can only return None if there are no futures. The stream mustn't be polled
        // after it has yielded None so we ensure that we do not attempt to poll in this case.
        if self.futures.is_empty() {
            return Poll::Pending;
        }
        while let Poll::Ready(result) = self.futures.poll_next_unpin(cx) {
            // Unwrap because we checked that it isn't empty.
            let result = result.unwrap();
            if self.futures.is_empty() || (self.filter)(&result) {
                self.is_terminated = true;
                return Poll::Ready(result);
            }
        }
        Poll::Pending
    }
}

// We implement this because this is useful for consumers for example when they want to use
// `select!`. Usually they would just call `FutureExt::fuse` but for this type it would prevent them
// from pushing more futures afterwards.
impl<Fut, Filter> FusedFuture for FirstMatchOrLast<Fut, Filter>
where
    Fut: Future,
    Filter: FnMut(&Fut::Output) -> bool + Unpin,
{
    fn is_terminated(&self) -> bool {
        self.is_terminated
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::future::{self, FutureExt as _};

    #[test]
    fn ready_because_filter() {
        let first_match = FirstMatchOrLast::new(|i: &u8| *i == 3);
        first_match.add(future::pending().boxed());
        first_match.add(future::ready(2).boxed());
        first_match.add(future::ready(3).boxed());
        assert_eq!(first_match.now_or_never().unwrap(), 3);
    }

    #[test]
    fn ready_because_last() {
        let first_match = FirstMatchOrLast::new(|i: &u8| *i == 3);
        first_match.add(future::ready(2).boxed());
        assert_eq!(first_match.now_or_never().unwrap(), 2);
    }

    #[test]
    fn not_ready_because_pending() {
        let first_match = FirstMatchOrLast::new(|_: &()| false);
        first_match.add(future::pending().boxed());
        assert_eq!(first_match.now_or_never(), None);
    }

    #[test]
    fn not_ready_before_push() {
        let first_match = FirstMatchOrLast::<future::Pending<()>, _>::new(|_: &()| false);
        assert_eq!(first_match.now_or_never(), None);
    }
}
