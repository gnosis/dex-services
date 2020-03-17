//! Module contains the implementation for a shared HTTP client for various
//! driver components.

use crate::metrics::{HttpMetrics, LabeledSubsystem, UnlabeledSubsystem};
use anyhow::{anyhow, Result};
use futures::future::{BoxFuture, FutureExt};
use isahc::http::{Error as HttpError, Uri};
use isahc::prelude::{Configurable, Request};
use isahc::{HttpClientBuilder, ResponseExt};
use serde::de::DeserializeOwned;
use std::convert::TryFrom;
use std::marker::PhantomData;
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
        HttpFactory::new(Duration::from_secs(10), HttpMetrics::default())
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
        Uri: TryFrom<U>,
        <Uri as TryFrom<U>>::Error: Into<HttpError>,
        T: DeserializeOwned,
    {
        let start = Instant::now();
        let json = self.inner.get(url)?.text()?;
        let size = json.len();
        let result = serde_json::from_str(&json)?;

        Ok((result, start.elapsed(), size))
    }

    /// Standard HTTP GET request that parses the result as JSON.
    async fn get_json_async<U, T>(&self, url: U) -> Result<(T, Duration, usize)>
    where
        Uri: TryFrom<U>,
        <Uri as TryFrom<U>>::Error: Into<HttpError>,
        T: DeserializeOwned,
    {
        let start = Instant::now();
        let json = self.inner.get_async(url).await?.text()?;
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

/// Trait for extending HTTP client with labeled version of HTTP methods with
/// metrics.
///
/// This trait allows `HttpClient` to require a label value when a subsystem is
/// labeled, and to not require a label value when it is unlabeled.
pub trait LabeledHttpClient<Subsystem> {
    /// Post raw JSON data and return a future that resolves once the HTTP
    /// request has been completed.
    fn post_raw_json_async<'a, U>(
        &'a self,
        url: U,
        data: impl Into<String>,
        label: Subsystem,
    ) -> BoxFuture<'a, Result<String>>
    where
        U: Send + 'a,
        Uri: TryFrom<U>,
        <Uri as TryFrom<U>>::Error: Into<HttpError>;

    /// Standard HTTP GET request that parses the result as JSON.
    fn get_json<U, T>(&self, url: U, label: Subsystem) -> Result<T>
    where
        Uri: TryFrom<U>,
        <Uri as TryFrom<U>>::Error: Into<HttpError>,
        T: DeserializeOwned;
}

impl<Subsystem> LabeledHttpClient<Subsystem> for HttpClient<Subsystem>
where
    Subsystem: LabeledSubsystem + Send + Sync + 'static,
{
    /// Post raw JSON data and return a future that resolves once the HTTP
    /// request has been completed.
    fn post_raw_json_async<'a, U>(
        &'a self,
        url: U,
        data: impl Into<String>,
        label: Subsystem,
    ) -> BoxFuture<'a, Result<String>>
    where
        U: Send + 'a,
        Uri: TryFrom<U>,
        <Uri as TryFrom<U>>::Error: Into<HttpError>,
    {
        let data = data.into();
        async move {
            let (value, latency) = self.inner.post_raw_json_async(url, data).await?;
            self.metrics.request(label, latency, value.len());

            Ok(value)
        }
        .boxed()
    }

    /// Standard HTTP GET request that parses the result as JSON.
    fn get_json<U, T>(&self, url: U, label: Subsystem) -> Result<T>
    where
        Uri: TryFrom<U>,
        <Uri as TryFrom<U>>::Error: Into<HttpError>,
        T: DeserializeOwned,
    {
        let (value, latency, size) = self.inner.get_json(url)?;
        self.metrics.request(label, latency, size);

        Ok(value)
    }
}

/// Trait for extending HTTP client with unlabeled version of HTTP methods with
/// metrics.
///
/// This trait allows `HttpClient` to require a label value when a subsystem is
/// labeled, and to not require a label value when it is unlabeled.
pub trait UnlabeledHttpClient<Subsystem> {
    /// Standard HTTP GET request that parses the result as JSON.
    fn get_json<U, T>(&self, url: U) -> Result<T>
    where
        Uri: TryFrom<U>,
        <Uri as TryFrom<U>>::Error: Into<HttpError>,
        T: DeserializeOwned;

    /// Async HTTP GET request that parses the result as JSON.
    fn get_json_async<'a, U, T>(&'a self, url: U) -> BoxFuture<'a, Result<T>>
    where
        U: Send + 'a,
        Uri: TryFrom<U>,
        <Uri as TryFrom<U>>::Error: Into<HttpError>,
        T: DeserializeOwned;
}

impl<Subsystem> UnlabeledHttpClient<Subsystem> for HttpClient<Subsystem>
where
    Subsystem: UnlabeledSubsystem + Send + Sync + 'static,
{
    /// Standard HTTP GET request that parses the result as JSON.
    fn get_json<U, T>(&self, url: U) -> Result<T>
    where
        Uri: TryFrom<U>,
        <Uri as TryFrom<U>>::Error: Into<HttpError>,
        T: DeserializeOwned,
    {
        let (value, latency, size) = self.inner.get_json(url)?;
        self.metrics.request(Subsystem::default(), latency, size);

        Ok(value)
    }

    /// Async HTTP GET request that parses the result as JSON.
    fn get_json_async<'a, U, T>(&'a self, url: U) -> BoxFuture<'a, Result<T>>
    where
        U: Send + 'a,
        Uri: TryFrom<U>,
        <Uri as TryFrom<U>>::Error: Into<HttpError>,
        T: DeserializeOwned,
    {
        async move {
            let (value, latency, size) = self.inner.get_json_async(url).await?;
            self.metrics.request(Subsystem::default(), latency, size);

            Ok(value)
        }
        .boxed()
    }
}
