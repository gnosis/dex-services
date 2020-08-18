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
