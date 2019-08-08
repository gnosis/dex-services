use std::error::Error;
use std::fmt;
use ethabi;

use dfusion_core::database::DatabaseError;

#[derive(Debug, Clone, PartialEq)]
pub enum ErrorKind {
    Unknown,
    MiscError,
    IoError,
    ContractError,
    JsonError,
    HexError,
    AbiError,
    EnvError,
    DbError,
    ParseIntError,
    StateError,
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
        DriverError::new(error.description(), ErrorKind::ContractError)
    }
}
impl From<web3::Error> for DriverError {
    fn from(error: web3::Error) -> Self {
        DriverError::new(error.description(), ErrorKind::ContractError)
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
impl From<ethabi::Error> for DriverError {
    fn from(error: ethabi::Error) -> Self {
        DriverError::new(error.description(), ErrorKind::AbiError)
    }
}
impl From<std::env::VarError> for DriverError {
    fn from(error: std::env::VarError) -> Self {
        DriverError::new(error.description(), ErrorKind::EnvError)
    }
}
impl From<mongodb::Error> for DriverError {
    fn from(error: mongodb::Error) -> Self {
        DriverError::new(error.description(), ErrorKind::DbError)
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

impl DriverError {
    pub fn new(msg: &str, kind: ErrorKind) -> DriverError {
        DriverError{details: msg.to_string(), kind}
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