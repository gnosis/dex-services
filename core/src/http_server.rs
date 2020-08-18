//! Module implementing an HTTP server for exposing and endpoints for health and
//! metrics data for general performance monitoring.

mod routing;

pub use self::routing::DefaultRouter;
use anyhow::Result;
use rouille::{Request, Response};
use std::{net::SocketAddr, thread};

/// The default port for hosting the service monitor HTTP server.
pub const DEFAULT_MONITOR_PORT: u16 = 9586;

/// Trait for serving an HTTP endpoint exposing service monitoring data.
pub trait Serving {
    /// Starts serving the HTTP server on the specified port.
    fn serve(self, port: u16) -> !;

    /// Starts the HTTP server on a background thread with the default monitor
    /// port.
    fn start_in_background(self)
    where
        Self: Sized + Send + 'static,
    {
        let _ = thread::spawn(move || self.serve(DEFAULT_MONITOR_PORT));
    }
}

/// A `rouille` based HTTP server.
pub struct RouilleServer {
    handler: Box<dyn Handler>,
}

impl RouilleServer {
    /// Creates a new `rouille` server with the specified handler.
    pub fn new(handler: impl Handler) -> Self {
        Self {
            handler: Box::new(handler),
        }
    }
}

impl Serving for RouilleServer {
    fn serve(self, port: u16) -> ! {
        let addr: SocketAddr = ([0, 0, 0, 0], port).into();
        rouille::start_server(addr, move |request| {
            let url = request.url();
            log::debug!("handling '{}' request to monitoring HTTP server", url);

            self.handler.handle_request(request).unwrap_or_else(|err| {
                log::warn!("error executing '{}' request: {:?}", url, err);
                Response::text("internal server error").with_status_code(500)
            })
        });
    }
}

/// An endpoint that can be registered on a path.
#[cfg_attr(test, mockall::automock)]
pub trait Handler: Send + Sync + 'static {
    /// Handles an HTTP request.
    fn handle_request(&self, request: &Request) -> Result<Response>;
}
