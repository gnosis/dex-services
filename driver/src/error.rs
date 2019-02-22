use std::error::Error;
use std::fmt;

#[derive(Debug)]
pub struct DriverError {
    details: String
}

impl DriverError {
    pub fn new(msg: &str) -> DriverError {
        DriverError{details: msg.to_string()}
    }
}
impl fmt::Display for DriverError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f,"{}",self.details)
    }
}
impl Error for DriverError {
    fn description(&self) -> &str {
        &self.details
    }
}