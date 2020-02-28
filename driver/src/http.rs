//! Module contains the implementation for a shared HTTP client for various
//! driver components.

use crate::metrics::{HttpMetrics, LabeledSubsystem, UnlabeledSubsystem};
use anyhow::{anyhow, Result};
use isahc::http::{Error as HttpError, Uri};
use isahc::prelude::{Configurable, Request};
use isahc::{HttpClientBuilder, ResponseExt};
use serde::de::DeserializeOwned;
use std::convert::TryFrom;
use std::time::Duration;
use std::marker::PhantomData;
use std::sync::Arc;
use std::time::{Duration, Instant};

#[cfg(test)]
lazy_static::lazy_static! {
    /// An HTTP factory used for testing.
    pub static ref TEST_FACTORY: HttpFactory = HttpFactory::new(Duration::from_secs(10), HttpMetrics::default());
}

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
    pub fn create<T>(&self) -> Result<HttpClient<T>> {
        self.with_config(|builder| builder.timeout(self.default_timeout))
    }

    /// Creates a new HTTP Client with the given configuration.
    pub fn with_config<T>(
        &self,
        configure: impl FnOnce(HttpClientBuilder) -> HttpClientBuilder,
    ) -> Result<HttpClient<T>> {
        let inner = configure(isahc::HttpClient::builder()).build()?;
        let metrics = self.metrics.clone();

        Ok(HttpClient {
            inner: HttpClientInner { inner },
            metrics,
            _subsystem: PhantomData,
        })
    }
}

#[cfg(test)]
impl Default for HttpFactory {
    fn default() -> Self {
        HttpFactory::new(Duration::from_secs(10))
    }
}

/// An HTTP client instance.
#[derive(Debug)]
pub struct HttpClientInner {
    inner: isahc::HttpClient,
}

impl HttpClientInner {
    /// Post raw JSON data and return a future that resolves once the HTTP
    /// request has been completed.
    async fn post_raw_json_async<U>(
        &self,
        url: U,
        data: impl Into<String>,
    ) -> Result<(String, Duration)>
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
            Ok((content, start.elapsed()))
        } else {
            Err(anyhow!(
                "HTTP error status {}: '{}'",
                response.status(),
                content.trim()
            ))
        }
    }

    /// Standard HTTP GET request that parses the result as JSON.
    fn get_json<U, T>(&self, url: U) -> Result<(T, Duration, usize)>
    where
        Uri: HttpTryFrom<U>,
        T: DeserializeOwned,
    {
        let start = Instant::now();
        let json = self.inner.get(url)?.text()?;
        let size = json.len();
        let result = serde_json::from_str(&json)?;

        Ok((result, start.elapsed(), size))
    }
}

/// An HTTP client instance with metrics.
#[derive(Debug)]
pub struct HttpClient<Subsystem> {
    inner: HttpClientInner,
    metrics: Arc<HttpMetrics>,
    _subsystem: PhantomData<Subsystem>,
}

impl<Subsystem: LabeledSubsystem + 'static> HttpClient<Subsystem> {
    /// Post raw JSON data and return a future that resolves once the HTTP
    /// request has been completed.
    pub async fn post_raw_json_async_labeled<U>(
        &self,
        url: U,
        data: impl Into<String>,
        label: Subsystem,
    ) -> Result<String>
    where
        Uri: HttpTryFrom<U>,
    {
        let (value, latency) = self.inner.post_raw_json_async(url, data).await?;
        self.metrics.request_labeled(label, latency, value.len());

        Ok(value)
    }

    /// Standard HTTP GET request that parses the result as JSON.
    pub fn get_json_labeled<U, T>(&self, url: U, label: Subsystem) -> Result<T>
    where
        Uri: TryFrom<U>,
        <Uri as TryFrom<U>>::Error: Into<HttpError>,
        T: DeserializeOwned,
    {
        let (value, latency, size) = self.inner.get_json(url)?;
        self.metrics.request_labeled(label, latency, size);

        Ok(value)
    }
}

impl<Subsystem: UnlabeledSubsystem + 'static> HttpClient<Subsystem> {
    /// Standard HTTP GET request that parses the result as JSON.
    pub fn get_json_unlabeled<U, T>(&self, url: U) -> Result<T>
    where
        Uri: HttpTryFrom<U>,
        T: DeserializeOwned,
    {
        let (value, latency, size) = self.inner.get_json(url)?;
        self.metrics.request_unlabeled::<Subsystem>(latency, size);

        Ok(value)
    }

    /// Async HTTP GET request that parses the result as JSON.
    pub async fn get_json_async<U, T>(&self, url: U) -> Result<T>
    where
        Uri: TryFrom<U>,
        <Uri as TryFrom<U>>::Error: Into<HttpError>,
        T: DeserializeOwned,
    {
        Ok(self.inner.get_async(url).await?.json()?)
    }
}
