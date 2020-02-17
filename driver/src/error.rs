use crate::price_finding::error::PriceFindingError;

use std::error::Error;
use std::fmt;

#[derive(Debug, Clone, PartialEq)]
pub enum ErrorKind {
    MiscError,
    IoError,
    Web3Error,
    JsonError,
    HexError,
    EnvError,
    ParseIntError,
    PriceFindingError,
    PrivateKeyError,
    ContractDeployedError,
    ContractMethodError,
    HttpError,
}

#[derive(Debug, Clone, PartialEq)]
pub struct DriverError {
    details: String,
    pub kind: ErrorKind,
}

impl From<std::io::Error> for DriverError {
    fn from(error: std::io::Error) -> Self {
        DriverError::new(error.description(), ErrorKind::IoError)
    }
}

impl From<ethcontract::web3::Error> for DriverError {
    fn from(error: ethcontract::web3::Error) -> Self {
        DriverError::new(&format!("{}", error), ErrorKind::Web3Error)
    }
}

impl From<serde_json::Error> for DriverError {
    fn from(error: serde_json::Error) -> Self {
        DriverError::new(error.description(), ErrorKind::JsonError)
    }
}

impl From<rustc_hex::FromHexError> for DriverError {
    fn from(error: rustc_hex::FromHexError) -> Self {
        DriverError::new(error.description(), ErrorKind::HexError)
    }
}

impl From<std::env::VarError> for DriverError {
    fn from(error: std::env::VarError) -> Self {
        DriverError::new(error.description(), ErrorKind::EnvError)
    }
}

impl From<std::num::ParseIntError> for DriverError {
    fn from(error: std::num::ParseIntError) -> Self {
        DriverError::new(error.description(), ErrorKind::ParseIntError)
    }
}

impl From<&str> for DriverError {
    fn from(error: &str) -> Self {
        DriverError::new(error, ErrorKind::MiscError)
    }
}

impl From<PriceFindingError> for DriverError {
    fn from(error: PriceFindingError) -> Self {
        DriverError::new(&format!("{}", error), ErrorKind::PriceFindingError)
    }
}

impl From<ethcontract::errors::InvalidPrivateKey> for DriverError {
    fn from(error: ethcontract::errors::InvalidPrivateKey) -> Self {
        DriverError::new(&error.to_string(), ErrorKind::PrivateKeyError)
    }
}

impl From<ethcontract::errors::DeployError> for DriverError {
    fn from(error: ethcontract::errors::DeployError) -> Self {
        DriverError::new(&error.to_string(), ErrorKind::ContractDeployedError)
    }
}

impl From<ethcontract::errors::MethodError> for DriverError {
    fn from(error: ethcontract::errors::MethodError) -> Self {
        DriverError::new(&error.to_string(), ErrorKind::ContractMethodError)
    }
}

impl From<isahc::Error> for DriverError {
    fn from(error: isahc::Error) -> Self {
        DriverError::new(&error.to_string(), ErrorKind::HttpError)
    }
}

impl DriverError {
    pub fn new(msg: &str, kind: ErrorKind) -> DriverError {
        DriverError {
            details: msg.to_string(),
            kind,
        }
    }
}

impl fmt::Display for DriverError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "Driver Error: [{}]", self.details)
    }
}

impl Error for DriverError {
    fn description(&self) -> &str {
        &self.details
    }
}
