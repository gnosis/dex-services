use crate::monitoring::Handler;
use anyhow::{Context as _, Result};
use prometheus::{Encoder, Registry, TextEncoder};
use rouille::{Request, Response};
use std::sync::Arc;

pub struct MetricsHandler {
    registry: Arc<Registry>,
    encoder: TextEncoder,
}

impl MetricsHandler {
    pub fn new(registry: Arc<Registry>) -> Self {
        Self {
            registry,
            encoder: TextEncoder::new(),
        }
    }

    /// Encodes the given registry and returns the content type with the encoded
    /// data bytes.
    pub fn encode(&self) -> Result<(String, Vec<u8>)> {
        let metric_families = self.registry.gather();
        let mut buffer = vec![];
        self.encoder
            .encode(&metric_families, &mut buffer)
            .context("Could not encode metrics")?;

        Ok((self.encoder.format_type().to_owned(), buffer))
    }
}

impl Handler for MetricsHandler {
    fn handle_request(&self, _: &Request) -> Result<Response> {
        let (content_type, bytes) = self.encode()?;
        Ok(Response::from_data(content_type, bytes))
    }
}
