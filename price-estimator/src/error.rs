//! Module implementing a custom warp rejection for internal service errors.

use anyhow::Error;
use warp::{
    http::StatusCode,
    reject::{self, Reject, Rejection},
};

/// An internal error type used for common rejections types.
#[derive(Debug)]
pub enum RejectionReason {
    /// No token information available for converting between atoms and base
    /// units.
    NoTokenInfo,
    /// The token symbol or address was not found.
    TokenNotFound,
    /// Internal server error.
    InternalError(Error),
}

impl RejectionReason {
    /// Retrieve an HTTP status code and error message for the given rejection
    /// reason.
    pub fn as_http_error(&self) -> (StatusCode, &'static str) {
        match self {
            RejectionReason::NoTokenInfo => (
                StatusCode::BAD_REQUEST,
                "requested base units for token with missing ERC20 info",
            ),
            RejectionReason::TokenNotFound => {
                (StatusCode::BAD_REQUEST, "token symbol or address not found")
            }
            RejectionReason::InternalError(_) => {
                (StatusCode::INTERNAL_SERVER_ERROR, "internal server error")
            }
        }
    }
}

impl Reject for RejectionReason {}

impl From<RejectionReason> for Rejection {
    fn from(reason: RejectionReason) -> Self {
        reject::custom(reason)
    }
}
