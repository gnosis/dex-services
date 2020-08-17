//! Module implementing warp filter for hosting hosting prometheus metrics.

use crate::error::RejectionReason;
use core::metrics::MetricsHandler;
use prometheus::Registry;
use std::sync::Arc;
use warp::{
    http::{header, Response, StatusCode},
    Filter, Rejection, Reply,
};

/// Filter for the specified prometheus metrics.
pub fn filter(
    registry: Arc<Registry>,
) -> impl Filter<Extract = impl Reply, Error = Rejection> + Clone {
    let metrics = Arc::new(MetricsHandler::new(registry));
    warp::path!("metrics")
        .and(warp::get())
        .and(warp::any().map(move || metrics.clone()))
        .and_then(|metrics| async move { encode_metrics(metrics) })
}

fn encode_metrics(metrics: Arc<MetricsHandler>) -> Result<impl Reply, Rejection> {
    let (content_type, body) = metrics.encode().map_err(RejectionReason::InternalError)?;
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, content_type)
        .body(body)
        .map_err(|err| RejectionReason::InternalError(err.into()).into())
}
