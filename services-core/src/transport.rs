use crate::http::{HttpClient, HttpFactory, HttpLabel};
use anyhow::Error;
use ethcontract::jsonrpc::types::{Call, Output, Request};
use ethcontract::web3::helpers;
use ethcontract::web3::{BatchTransport, Error as Web3Error, RequestId, Transport};
use futures::future::{BoxFuture, FutureExt};
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
    client: HttpClient,
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

type RpcResult = Result<Value, Web3Error>;

impl HttpTransportInner {
    /// Execute an HTTP JSON RPC request.
    async fn execute_rpc(self: Arc<Self>, id: RequestId, request: Request) -> RpcResult {
        let label: HttpLabel = (&request).into();

        let request = serde_json::to_string(&request)?;
        debug!("[id:{}] sending request: '{}'", id, &request);

        let content = self
            .client
            .post_raw_json_async(&self.url, request, label)
            .await
            .map_err(|err| {
                warn!("[id:{}] returned an error: '{}'", id, err.to_string());
                Web3Error::Transport(err.to_string())
            })?;

        debug!("[id:{}] received response: '{}'", id, content.trim());
        let mut json = Value::from_str(&content)?;
        if let Some(map) = json.as_object_mut() {
            // NOTE: Ganache sometimes returns errors inline with responses,
            //   filter those out.
            if map.contains_key("result") {
                if let Some(error) = map.remove("error") {
                    info!("[id:{}] received Ganache auxiliary error {}", id, error);
                }
            }
        }

        Ok(json)
    }

    async fn execute_single_rpc(self: Arc<Self>, id: RequestId, call: Call) -> RpcResult {
        let json = self.execute_rpc(id, Request::Single(call)).await?;
        let output = Output::deserialize(json)?;
        let result = helpers::to_result_from_output(output)?;
        Ok(result)
    }

    async fn execute_batch_rpc(
        self: Arc<Self>,
        id: RequestId,
        request: Vec<Call>,
    ) -> Result<Vec<RpcResult>, Web3Error> {
        let result = self.execute_rpc(id, Request::Batch(request)).await?;
        let sub_results = result.as_array().ok_or_else(|| {
            warn!(
                "[id:{}] Batch request did not return a list of responses: '{}'",
                id,
                result.to_string()
            );
            Web3Error::InvalidResponse(result.to_string())
        })?;
        Ok(sub_results
            .iter()
            .map(|result| {
                let output = Output::deserialize(result)?;
                let result = helpers::to_result_from_output(output)?;
                Ok(result)
            })
            .collect())
    }
}

impl Debug for HttpTransport {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        f.debug_tuple("HttpTransport").field(&self.0.url).finish()
    }
}

impl Transport for HttpTransport {
    type Out = BoxFuture<'static, RpcResult>;

    fn prepare(&self, method: &str, params: Vec<Value>) -> (RequestId, Call) {
        let id = self.0.id.fetch_add(1, Ordering::SeqCst);
        let request = helpers::build_request(id, method, params);

        (id, request)
    }

    fn send(&self, id: RequestId, request: Call) -> Self::Out {
        self.0.clone().execute_single_rpc(id, request).boxed()
    }
}

impl BatchTransport for HttpTransport {
    type Batch = BoxFuture<'static, Result<Vec<RpcResult>, Web3Error>>;

    fn send_batch<T>(&self, requests: T) -> Self::Batch
    where
        T: IntoIterator<Item = (RequestId, Call)>,
    {
        let id = self.0.id.fetch_add(1, Ordering::SeqCst);
        let requests = requests.into_iter().map(|r| r.1).collect();
        self.0.clone().execute_batch_rpc(id, requests).boxed()
    }
}
