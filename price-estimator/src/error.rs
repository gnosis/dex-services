//! Module implementing a custom warp rejection for internal service errors.

use anyhow::Error;
use warp::reject::{self, Reject, Rejection};

#[derive(Debug)]
pub struct InternalError(pub Error);

impl Reject for InternalError {}

/// Create a
pub fn internal_server_rejection(err: Error) -> Rejection {
    reject::custom(InternalError(err))
}
