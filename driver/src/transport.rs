use crate::error::DriverError;
use ethcontract::jsonrpc::types::{Call, Output};
use ethcontract::web3::helpers;
use ethcontract::web3::{Error as Web3Error, RequestId, Transport};
use futures::compat::Compat;
use futures::future::{BoxFuture, FutureExt, TryFutureExt};
use isahc::{HttpClient, ResponseExt};
use log::{log, Level};
use serde::Deserialize;
use serde_json::Value;
use std::fmt::{self, Debug, Formatter};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

/// An HTTP transport implementation with timeout and logging.
#[derive(Clone)]
pub struct HttpTransport(Arc<HttpTransportInner>);

struct HttpTransportInner {
    url: String,
    log_level: Level,
    client: HttpClient,
    id: AtomicUsize,
}

impl HttpTransport {
    /// Creates a new HTTP transport with settings.
    pub fn new(
        url: impl Into<String>,
        log_level: Level,
        timeout: Duration,
    ) -> Result<HttpTransport, DriverError> {
        let client = HttpClient::builder().timeout(timeout).build()?;

        Ok(HttpTransport(Arc::new(HttpTransportInner {
            url: url.into(),
            log_level,
            client,
            id: AtomicUsize::default(),
        })))
    }
}

impl HttpTransportInner {
    async fn execute(self: Arc<Self>, id: RequestId, request: Call) -> Result<Value, Web3Error> {
        let request = serde_json::to_string(&request)?;
        log!(self.log_level, "sending request ID {}: {}", id, &request);

        let mut response: Value = self
            .client
            .post_async(&self.url, request)
            .await
            .map_err(|err| Web3Error::Transport(err.to_string()))?
            .json()?;
        log!(self.log_level, "received response ID {}: {}", id, &response);

        if let Some(map) = response.as_object_mut() {
            // NOTE: Ganache sometimes returns errors inlined with responses,
            //   filter those out.
            if map.contains_key("result") && map.contains_key("error") {
                map.remove("error");
            }
        }

        let output = Output::deserialize(response)?;
        let result = helpers::to_result_from_output(output)?;

        Ok(result)
    }
}

impl Debug for HttpTransport {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        f.debug_tuple("HttpTransport").field(&self.0.url).finish()
    }
}

impl Transport for HttpTransport {
    type Out = Compat<BoxFuture<'static, Result<Value, Web3Error>>>;

    fn prepare(&self, method: &str, params: Vec<Value>) -> (RequestId, Call) {
        let id = self.0.id.fetch_add(1, Ordering::SeqCst);
        let request = helpers::build_request(id, method, params);

        (id, request)
    }

    fn send(&self, id: RequestId, request: Call) -> Self::Out {
        let send = self.0.clone().execute(id, request);
        send.boxed().compat()
    }
}
