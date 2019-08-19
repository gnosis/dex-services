use std::fmt;

#[derive(Debug, Clone, PartialEq)]
pub struct DatabaseError {
    details: String,
    kind: ErrorKind,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ErrorKind {
    Unknown,
    ConfigurationError,
    ConnectionError,
    StateError,
}

impl fmt::Display for DatabaseError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "Database Error: [{}]", self.details)
    }
}

impl DatabaseError {
    pub fn new(kind: ErrorKind, description: &str) -> Self {
        DatabaseError {
            details: description.to_string(),
            kind,
        }
    }

    pub fn chain<T: fmt::Display>(kind: ErrorKind, description: &str, underlying_error: T) -> Self {
        DatabaseError {
            details: format!("{} - {}", description, underlying_error),
            kind,
        }
    }
}
