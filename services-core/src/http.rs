//! Module contains the implementation for a shared HTTP client for various
//! driver components.

pub use crate::metrics::HttpLabel;
use crate::metrics::HttpMetrics;
use anyhow::{anyhow, Context, Result};
use isahc::http::{Error as HttpError, Uri};
use isahc::prelude::{Configurable, Request};
use isahc::{HttpClientBuilder, ResponseExt};
use serde::de::DeserializeOwned;
use std::convert::TryFrom;
use std::sync::Arc;
use std::time::{Duration, Instant};

/// A factory type for creating HTTP clients.
#[derive(Debug)]
pub struct HttpFactory {
    default_timeout: Duration,
    metrics: Arc<HttpMetrics>,
}

impl HttpFactory {
    /// Creates a new HTTP client factory.
    pub fn new(default_timeout: Duration, metrics: HttpMetrics) -> Self {
        HttpFactory {
            default_timeout,
            metrics: Arc::new(metrics),
        }
    }

    /// Creates a new HTTP client with the default configuration.
    pub fn create(&self) -> Result<HttpClient> {
        self.with_config(|builder| builder.timeout(self.default_timeout))
    }

    /// Creates a new HTTP Client with the given configuration.
    pub fn with_config(
        &self,
        configure: impl FnOnce(HttpClientBuilder) -> HttpClientBuilder,
    ) -> Result<HttpClient> {
        let inner = configure(isahc::HttpClient::builder()).build()?;
        let metrics = self.metrics.clone();

        Ok(HttpClient { inner, metrics })
    }
}

impl Default for HttpFactory {
    fn default() -> Self {
        HttpFactory::new(Duration::from_secs(10), HttpMetrics::default())
    }
}

/// An HTTP client instance with metrics.
#[derive(Debug)]
pub struct HttpClient {
    inner: isahc::HttpClient,
    metrics: Arc<HttpMetrics>,
}

impl HttpClient {
    /// Post raw JSON data and return a future that resolves once the HTTP
    /// request has been completed.
    pub async fn post_raw_json_async<U>(
        &self,
        url: U,
        data: impl Into<String>,
        label: HttpLabel,
    ) -> Result<String>
    where
        Uri: TryFrom<U>,
        <Uri as TryFrom<U>>::Error: Into<HttpError>,
    {
        let start = Instant::now();
        let http_request = Request::post(url)
            .header("Content-Type", "application/json")
            .body(data.into())?;
        let mut response = self.inner.send_async(http_request).await?;
        let content = response.text()?;

        if response.status().is_success() {
            self.metrics.request(label, start.elapsed(), content.len());
            Ok(content)
        } else {
            Err(anyhow!(
                "HTTP error status {}: '{}'",
                response.status(),
                content.trim()
            ))
        }
    }

    /// Standard HTTP GET request that parses the result as JSON.
    pub async fn get_json_async<U, T>(&self, url: U, label: HttpLabel) -> Result<T>
    where
        Uri: TryFrom<U>,
        <Uri as TryFrom<U>>::Error: Into<HttpError>,
        T: DeserializeOwned,
    {
        let start = Instant::now();

        let json = self.inner.get_async(url).await?.text()?;
        let size = json.len();
        self.metrics.request(label, start.elapsed(), size);

        let result = serde_json::from_str(&json)
            .with_context(|| format!("failed to parse JSON '{}'", json))?;
        Ok(result)
    }
}
