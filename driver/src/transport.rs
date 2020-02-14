use crate::error::DriverError;
use ethcontract::jsonrpc::types::{Call, Output};
use ethcontract::web3::helpers;
use ethcontract::web3::{Error as Web3Error, RequestId, Transport};
use futures::compat::Compat;
use futures::future::{BoxFuture, FutureExt, TryFutureExt};
use isahc::config::VersionNegotiation;
use isahc::prelude::{HttpClient, Request, ResponseExt};
use log::{log, warn, Level};
use serde::Deserialize;
use serde_json::Value;
use std::fmt::{self, Debug, Formatter};
use std::str::FromStr;
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
        let client = HttpClient::builder()
            .timeout(timeout)
            .version_negotiation(VersionNegotiation::http11())
            .build()?;

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
        log!(
            self.log_level,
            "[id:{}] sending request: '{}'",
            id,
            &request,
        );

        let http_request = Request::post(&self.url)
            .header("Content-Type", "application/json")
            .body(request)
            .map_err(|err| Web3Error::Transport(err.to_string()))?;
        let mut response = self
            .client
            .send_async(http_request)
            .await
            .map_err(|err| Web3Error::Transport(err.to_string()))?;
        let body = response
            .text()
            .map_err(|err| Web3Error::Transport(err.to_string()))?;

        if !response.status().is_success() {
            warn!(
                "[id:{}] HTTP error code {}: '{}' {:?}",
                id,
                response.status(),
                body.trim(),
                response,
            );
            return Err(Web3Error::Transport(format!(
                "HTTP error status {}: '{}'",
                response.status(),
                body.trim(),
            )));
        }
        log!(self.log_level, "received response ID {}: '{}'", id, &body);

        let mut json = Value::from_str(&body)?;
        if let Some(map) = json.as_object_mut() {
            // NOTE: Ganache sometimes returns errors inlined with responses,
            //   filter those out.
            if map.contains_key("result") {
                if let Some(error) = map.remove("error") {
                    warn!("[id:{}] received Ganache auxiliary error {}", id, error);
                }
            }
        }

        let output = Output::deserialize(json)?;
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
