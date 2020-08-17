//! Module implementing HTTP server for serving up health and metrics data for
//! general performance monitoring.

use anyhow::Result;
use rouille::router;
use rouille::{Request, Response};
use std::{net::SocketAddr, sync::Arc, thread};

/// Trait for serving an HTTP endpoint exposing service monitoring data.
pub trait Serving {
    /// Starts serving the HTTP server on the specified port.
    fn serve(self, port: u16) -> !;

    /// Starts the HTTP server on a background thread.
    fn start_in_background(self, port: u16)
    where
        Self: Sized + Send + 'static,
    {
        let _ = thread::spawn(move || self.serve(port));
    }
}

/// A `rouille` based HTTP server.
pub struct RouilleServer {
    health: Arc<dyn Handler>,
    metrics: Arc<dyn Handler>,
}

impl RouilleServer {
    /// Creates a new `rouille` server with the specified handler.
    pub fn new(health: Arc<dyn Handler>, metrics: Arc<dyn Handler>) -> Self {
        Self { health, metrics }
    }

    fn handle_request(&self, request: &Request) -> Response {
        let url = request.url();
        log::debug!("handling '{}' request to monitoring HTTP server", url);

        let handler = router!(request,
            (GET) (/health/readiness) => {
                self.health.as_ref()
            },
            (GET) (/metrics) => {
                self.metrics.as_ref()
            },
            _ => &NotFound,
        );

        handler.handle_request(request).unwrap_or_else(|err| {
            log::warn!("error executing '{}' request: {:?}", url, err);
            Response::text("internal server error").with_status_code(500)
        })
    }
}

impl Serving for RouilleServer {
    fn serve(self, port: u16) -> ! {
        let addr: SocketAddr = ([0, 0, 0, 0], port).into();
        rouille::start_server(addr, move |request| self.handle_request(request));
    }
}

/// An endpoint that can be registered on a path.
pub trait Handler: Send + Sync + 'static {
    /// Handles an HTTP request.
    fn handle_request(&self, request: &Request) -> Result<Response>;
}

/// Enpoint that always returns a 404 not-found response.
struct NotFound;

impl Handler for NotFound {
    fn handle_request(&self, _: &Request) -> Result<Response> {
        Ok(Response::empty_404())
    }
}
