use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use hyper::service::{make_service_fn, service_fn};
use hyper::{Body, Error, Response, Server};
use prometheus::{Encoder, Registry, TextEncoder};

pub struct MetricsServer {
    registry: Arc<Registry>,
}

impl MetricsServer {
    pub fn new(registry: Arc<Registry>) -> Self {
        Self { registry }
    }
    pub fn serve(&self, port: u16) -> Pin<Box<dyn Future<Output = Result<(), Error>> + Send>> {
        let addr = ([0, 0, 0, 0], port).into();
        let registry = self.registry.clone();
        let service = make_service_fn(move |_| {
            let registry = registry.clone();
            async move {
                Ok::<_, Error>(service_fn(move |_req| {
                    let registry = registry.clone();
                    async move {
                        let encoder = TextEncoder::new();
                        let metric_families = registry.gather();
                        let mut buffer = vec![];
                        encoder
                            .encode(&metric_families, &mut buffer)
                            .expect("Could not encode metrics");
                        Response::builder()
                            .status(200)
                            .header(hyper::header::CONTENT_TYPE, encoder.format_type())
                            .body(Body::from(buffer))
                    }
                }))
            }
        });
        Box::pin(Server::bind(&addr).serve(service))
    }
}
