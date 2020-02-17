use ethcontract::jsonrpc::types::request::Call;
use ethcontract::web3::error::Error;
use ethcontract::web3::futures::{Async, Future, Poll};
use ethcontract::web3::{RequestId, Transport};
use log::{log, Level};
use serde_json::Value;

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
        log!(
            self.level,
            "sending request ID {}: {}",
            id,
            serde_json::to_string(&request).expect("request is invalid JSON")
        );
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
        let result = self.inner.poll();
        match &result {
            Ok(Async::Ready(ref value)) => log!(
                self.level,
                "request ID {} completed with result: {}",
                self.id,
                value
            ),
            Err(ref err) => log!(self.level, "request ID {} failed: {:?}", self.id, err),
            _ => {}
        }
        result
    }
}
