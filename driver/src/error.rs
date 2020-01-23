use crate::price_finding::error::PriceFindingError;

use ethcontract::ethsign;
use std::error::Error;
use std::fmt;

use dfusion_core::database::DatabaseError;

#[derive(Debug, Clone, PartialEq)]
pub enum ErrorKind {
    Unknown,
    MiscError,
    IoError,
    ContractError,
    JsonError,
    HexError,
    EnvError,
    DbError,
    ParseIntError,
    StateError,
    PriceFindingError,
    SigningError,
    ContractDeployedError,
    ContractExecutionError,
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

impl From<web3::contract::Error> for DriverError {
    fn from(error: web3::contract::Error) -> Self {
        DriverError::new(&format!("{}", error), ErrorKind::ContractError)
    }
}

impl From<web3::Error> for DriverError {
    fn from(error: web3::Error) -> Self {
        DriverError::new(&format!("{}", error), ErrorKind::ContractError)
    }
}

impl From<serde_json::Error> for DriverError {
    fn from(error: serde_json::Error) -> Self {
        DriverError::new(error.description(), ErrorKind::JsonError)
    }
}

impl From<hex::FromHexError> for DriverError {
    fn from(error: hex::FromHexError) -> Self {
        DriverError::new(error.description(), ErrorKind::HexError)
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

impl From<DatabaseError> for DriverError {
    fn from(error: DatabaseError) -> Self {
        DriverError::new(&format!("{}", error), ErrorKind::DbError)
    }
}

impl From<PriceFindingError> for DriverError {
    fn from(error: PriceFindingError) -> Self {
        DriverError::new(&format!("{}", error), ErrorKind::PriceFindingError)
    }
}

impl From<ethsign::Error> for DriverError {
    fn from(error: ethsign::Error) -> Self {
        DriverError::new(&error.to_string(), ErrorKind::SigningError)
    }
}

impl From<ethcontract::errors::DeployError> for DriverError {
    fn from(error: ethcontract::errors::DeployError) -> Self {
        DriverError::new(&error.to_string(), ErrorKind::ContractDeployedError)
    }
}

impl From<ethcontract::errors::ExecutionError> for DriverError {
    fn from(error: ethcontract::errors::ExecutionError) -> Self {
        DriverError::new(&error.to_string(), ErrorKind::ContractExecutionError)
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
