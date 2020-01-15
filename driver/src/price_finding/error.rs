use std::error::Error;
use std::fmt;

#[derive(Debug, Clone, PartialEq)]
pub enum ErrorKind {
    Unknown,
    IoError,
    ExecutionError,
    JsonError,
    ParseIntError,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PriceFindingError {
    details: String,
    pub kind: ErrorKind,
}

impl From<std::io::Error> for PriceFindingError {
    fn from(error: std::io::Error) -> Self {
        PriceFindingError::new(error.description(), ErrorKind::IoError)
    }
}
impl From<serde_json::Error> for PriceFindingError {
    fn from(error: serde_json::Error) -> Self {
        PriceFindingError::new(error.description(), ErrorKind::JsonError)
    }
}
impl From<std::num::ParseIntError> for PriceFindingError {
    fn from(error: std::num::ParseIntError) -> Self {
        PriceFindingError::new(error.description(), ErrorKind::ParseIntError)
    }
}
impl From<&str> for PriceFindingError {
    fn from(error: &str) -> Self {
        PriceFindingError::new(error, ErrorKind::Unknown)
    }
}

impl PriceFindingError {
    pub fn new(msg: &str, kind: ErrorKind) -> PriceFindingError {
        PriceFindingError {
            details: msg.to_string(),
            kind,
        }
    }
}
impl fmt::Display for PriceFindingError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", self.details)
    }
}
impl Error for PriceFindingError {
    fn description(&self) -> &str {
        &self.details
    }
}
