use crate::http::{HttpClient, HttpFactory};
use crate::metrics::Web3Label;
use anyhow::Error;
use ethcontract::jsonrpc::types::{Call, Output};
use ethcontract::web3::helpers;
use ethcontract::web3::{Error as Web3Error, RequestId, Transport};
use futures::compat::Compat;
use futures::future::{BoxFuture, FutureExt, TryFutureExt};
use isahc::config::{Configurable, VersionNegotiation};
use log::{debug, info, warn};
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
    client: HttpClient<Web3Label>,
    id: AtomicUsize,
}

impl HttpTransport {
    /// Creates a new HTTP transport with settings.
    pub fn new(
        http_factory: &HttpFactory,
        url: impl Into<String>,
        timeout: Duration,
    ) -> Result<HttpTransport, Error> {
        let client = http_factory.with_config(|builder| {
            builder
                .timeout(timeout)
                // NOTE: This is needed as curl will try to upgrade to HTTP/2
                //   which causes a HTTP 400 error with Ganache.
                .version_negotiation(VersionNegotiation::http11())
        })?;

        Ok(HttpTransport(Arc::new(HttpTransportInner {
            url: url.into(),
            client,
            id: AtomicUsize::default(),
        })))
    }
}

impl HttpTransportInner {
    /// Execute an HTTP JSON RPC request.
    async fn execute_rpc(
        self: Arc<Self>,
        id: RequestId,
        request: Call,
    ) -> Result<Value, Web3Error> {
        let label = match &request {
            Call::MethodCall(call) if call.method == "eth_call" => Web3Label::Call,
            Call::MethodCall(call) if call.method == "eth_estimateGas" => Web3Label::EstimateGas,
            _ => Web3Label::Other,
        };

        let request = serde_json::to_string(&request)?;
        debug!("[id:{}] sending request: '{}'", id, &request);

        let content = self
            .client
            .post_raw_json_async_labeled(&self.url, request, label)
            .await
            .map_err(|err| {
                warn!("[id:{}] returned an error: '{}'", id, err.to_string());
                Web3Error::Transport(err.to_string())
            })?;

        debug!("[id:{}] received response: '{}'", id, content.trim());
        let mut json = Value::from_str(&content)?;
        if let Some(map) = json.as_object_mut() {
            // NOTE: Ganache sometimes returns errors inlined with responses,
            //   filter those out.
            if map.contains_key("result") {
                if let Some(error) = map.remove("error") {
                    info!("[id:{}] received Ganache auxiliary error {}", id, error);
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
        self.0.clone().execute_rpc(id, request).boxed().compat()
    }
}
