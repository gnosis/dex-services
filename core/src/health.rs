//! Module implementing shared basic health reporting.

use crate::http_server::Handler;
use anyhow::Result;
use rouille::{Request, Response};
use std::sync::atomic::{AtomicBool, Ordering};

/// Trait for asyncronously notifying health information.
#[cfg_attr(test, mockall::automock)]
pub trait HealthReporting: Send + Sync {
    /// Notify that the service is ready.
    fn notify_ready(&self);
}

/// Implementation sharing health information over an HTTP endpoint.
#[derive(Debug, Default)]
pub struct HttpHealthEndpoint {
    ready: AtomicBool,
}

impl HttpHealthEndpoint {
    /// Creates a new HTTP health enpoint.
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns true if the service is ready, false otherwise.
    fn is_ready(&self) -> bool {
        self.ready.load(Ordering::Acquire)
    }
}

impl HealthReporting for HttpHealthEndpoint {
    fn notify_ready(&self) {
        self.ready.store(true, Ordering::Release);
    }
}

impl Handler for HttpHealthEndpoint {
    fn handle_request(&self, _: &Request) -> Result<Response> {
        Ok(if self.is_ready() {
            Response::empty_204()
        } else {
            Response::text("service unavailable").with_status_code(503)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn responds_with_204_when_ready() {
        let health = HttpHealthEndpoint::new();
        health.notify_ready();

        let response = health
            .handle_request(&Request::fake_http(
                "GET",
                "/health/readiness",
                vec![],
                vec![],
            ))
            .unwrap();
        assert_eq!(response.status_code, 204);
    }

    #[test]
    fn responds_with_503_when_not_ready() {
        let health = HttpHealthEndpoint::new();

        let response = health
            .handle_request(&Request::fake_http(
                "GET",
                "/health/readiness",
                vec![],
                vec![],
            ))
            .unwrap();
        assert_eq!(response.status_code, 503);
    }
}
