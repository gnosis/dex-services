use crate::http_server::Handler;
use anyhow::{Context as _, Result};
use prometheus::{Encoder, Registry, TextEncoder};
use rouille::{Request, Response};
use std::sync::Arc;

pub struct MetricsHandler {
    registry: Arc<Registry>,
    encoder: TextEncoder,
}

impl MetricsHandler {
    /// Creates a new metrics handler from the specified registry using the
    /// default metrics data text encoding.
    pub fn new(registry: Arc<Registry>) -> Self {
        Self {
            registry,
            encoder: TextEncoder::new(),
        }
    }
}

impl Handler for MetricsHandler {
    fn handle_request(&self, _: &Request) -> Result<Response> {
        let metric_families = self.registry.gather();
        let mut buffer = vec![];
        self.encoder
            .encode(&metric_families, &mut buffer)
            .context("Could not encode metrics")?;

        Ok(Response::from_data(
            self.encoder.format_type().to_owned(),
            buffer,
        ))
    }
}
