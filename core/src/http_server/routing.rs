//! Module implementing the default HTTP router for the monitoring endpoints.

use super::Handler;
use anyhow::Result;
use rouille::{router, Request, Response};
use std::sync::Arc;

pub struct DefaultRouter {
    pub metrics: Arc<dyn Handler>,
    pub health_readiness: Arc<dyn Handler>,
}

impl Handler for DefaultRouter {
    fn handle_request(&self, request: &Request) -> Result<Response> {
        let handler = router!(request,
            (GET) (/metrics) => { self.metrics.as_ref() },
            (GET) (/health/readiness) => { self.health_readiness.as_ref() },
            _ => &NotFound,
        );
        handler.handle_request(request)
    }
}

/// Enpoint that always returns a 404 not-found response.
struct NotFound;

impl Handler for NotFound {
    fn handle_request(&self, _: &Request) -> Result<Response> {
        Ok(Response::empty_404())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http_server::MockHandler;

    #[test]
    fn routes_requests() {
        let mut metrics = MockHandler::new();
        metrics
            .expect_handle_request()
            .return_once(|_| Ok(Response::text("metrics").with_status_code(200)));

        let mut health_readiness = MockHandler::new();
        health_readiness
            .expect_handle_request()
            .return_once(|_| Ok(Response::text("health/readiness").with_status_code(204)));

        let router = DefaultRouter {
            metrics: Arc::new(metrics),
            health_readiness: Arc::new(health_readiness),
        };

        let response = router
            .handle_request(&Request::fake_http("GET", "/metrics", vec![], vec![]))
            .unwrap();
        assert_eq!(response.status_code, 200);

        let response = router
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
    fn returns_not_found_for_other_urls() {
        let router = DefaultRouter {
            metrics: Arc::new(MockHandler::new()),
            health_readiness: Arc::new(MockHandler::new()),
        };

        let response = router
            .handle_request(&Request::fake_http("GET", "/foo", vec![], vec![]))
            .unwrap();
        assert_eq!(response.status_code, 404);
    }
}
