use std::net::SocketAddr;
use std::sync::Arc;

use prometheus::{Encoder, Registry, TextEncoder};
use rouille::{start_server, Response};

pub struct MetricsServer {
    registry: Arc<Registry>,
}

impl MetricsServer {
    pub fn new(registry: Arc<Registry>) -> Self {
        Self { registry }
    }
    pub fn serve(&self, port: u16) {
        let addr: SocketAddr = ([0, 0, 0, 0], port).into();
        let registry = self.registry.clone();
        let encoder = TextEncoder::new();
        start_server(addr, move |_| {
            let metric_families = registry.gather();
            let mut buffer = vec![];
            encoder
                .encode(&metric_families, &mut buffer)
                .expect("Could not encode metrics");
            Response::from_data(encoder.format_type().to_owned(), buffer)
        })
    }
}
