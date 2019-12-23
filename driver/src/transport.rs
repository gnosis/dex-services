use jsonrpc_core::types::request::Call;
use log::{log, Level};
use serde_json::Value;
use web3::error::Error;
use web3::futures::{Async, Future, Poll};
use web3::{RequestId, Transport};

/// A `Transport` wrapper that logs RPC messages
#[derive(Clone, Debug)]
pub struct LoggingTransport<T> {
    inner: T,
    level: Level,
}

impl<T> LoggingTransport<T>
where
    T: Transport,
{
    /// Create a new `LoggingTranport` from an underlying transport and log
    /// level.
    pub fn new(inner: T, level: Level) -> Self {
        LoggingTransport { inner, level }
    }
}

impl<T> Transport for LoggingTransport<T>
where
    T: Transport,
{
    type Out = LoggingFuture<T::Out>;

    fn prepare(&self, method: &str, params: Vec<Value>) -> (RequestId, Call) {
        self.inner.prepare(method, params)
    }

    fn send(&self, id: RequestId, request: Call) -> Self::Out {
        log!(self.level, "sending request ID {}: {:?}", id, request);
        LoggingFuture {
            inner: self.inner.send(id, request),
            id,
            level: self.level,
        }
    }
}

/// A future that wraps JSON RPC results.
pub struct LoggingFuture<F> {
    inner: F,
    id: RequestId,
    level: Level,
}

impl<F> Future for LoggingFuture<F>
where
    F: Future<Item = Value, Error = Error>,
{
    type Item = Value;
    type Error = Error;

    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        match self.inner.poll() {
            Ok(Async::NotReady) => Ok(Async::NotReady),
            result => {
                log!(
                    self.level,
                    "request ID {} completed with result: {:?}",
                    self.id,
                    result
                );
                result
            }
        }
    }
}
