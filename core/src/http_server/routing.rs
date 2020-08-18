//! Module implementing the default HTTP router for the monitoring endpoints.

use super::Handler;
use crate::metrics::MetricsHandler;
use anyhow::Result;
use rouille::{router, Request, Response};

pub struct DefaultRouter(pub MetricsHandler);

impl Handler for DefaultRouter {
    fn handle_request(&self, request: &Request) -> Result<Response> {
        let Self(metrics) = self;

        let handler = router!(request,
            (GET) (/metrics) => { &*metrics as &dyn Handler },
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

    impl Default for DefaultRouter {
        fn default() -> Self {
            Self(MetricsHandler::new(Default::default()))
        }
    }

    #[test]
    fn returns_metrics() {
        let response = DefaultRouter::default()
            .handle_request(&Request::fake_http("GET", "/metrics", vec![], vec![]))
            .unwrap();
        assert!(response.is_success());
    }

    #[test]
    fn returns_not_found_for_other_urls() {
        let response = DefaultRouter::default()
            .handle_request(&Request::fake_http("GET", "/foo", vec![], vec![]))
            .unwrap();
        assert_eq!(response.status_code, 404);
    }
}
