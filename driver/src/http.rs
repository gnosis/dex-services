//! Module contains the implementation for a shared HTTP client for various
//! driver components.

use anyhow::{anyhow, Result};
use isahc::http::{HttpTryFrom, Uri};
use isahc::prelude::Request;
use isahc::{HttpClientBuilder, ResponseExt};
use serde::de::DeserializeOwned;
use std::time::Duration;

/// A factory type for creating HTTP clients.
#[derive(Debug)]
pub struct HttpFactory {
    default_timeout: Duration,
}

impl HttpFactory {
    /// Creates a new HTTP client factory.
    pub fn new(default_timeout: Duration) -> Self {
        HttpFactory { default_timeout }
    }

    /// Creates a new HTTP client with the default configuration.
    pub fn create(&self) -> Result<HttpClient> {
        self.with_config(|builder| builder)
    }

    /// Creates a new HTTP Client with the given configuration.
    pub fn with_config(
        &self,
        configure: impl FnOnce(HttpClientBuilder) -> HttpClientBuilder,
    ) -> Result<HttpClient> {
        let inner = configure(isahc::HttpClient::builder()).build()?;
        Ok(HttpClient { inner })
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
pub struct HttpClient {
    inner: isahc::HttpClient,
}

impl HttpClient {
    /// Post raw JSON data and return a future that resolves once the HTTP
    /// request has been completed.
    pub async fn post_raw_json_async<U>(&self, url: U, data: impl Into<String>) -> Result<String>
    where
        Uri: HttpTryFrom<U>,
    {
        let http_request = Request::post(url)
            .header("Content-Type", "application/json")
            .body(data.into())?;
        let mut response = self.inner.send_async(http_request).await?;
        let content = response.text()?;

        if response.status().is_success() {
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
    pub fn get_json<U, T>(&self, url: U) -> Result<T>
    where
        Uri: HttpTryFrom<U>,
        T: DeserializeOwned,
    {
        Ok(self.inner.get(url)?.json()?)
    }

    /// Async HTTP GET request that parses the result as JSON.
    pub async fn get_json_async<U, T>(&self, url: U) -> Result<T>
    where
        Uri: HttpTryFrom<U>,
        T: DeserializeOwned,
    {
        Ok(self.inner.get_async(url).await?.json()?)
    }
}
