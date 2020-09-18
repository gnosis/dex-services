//! Time utilites for dealing with batches.

use std::time::{Duration, SystemTime, SystemTimeError};

/// `SystemTime` extention trait.
pub trait SystemTimeExt {
    /// Creates a new `SystemTime` from a Unix timestamp.
    fn from_timestamp(timestamp: u64) -> SystemTime {
        SystemTime::UNIX_EPOCH + Duration::from_secs(timestamp)
    }

    /// Creates a new `SystemTime` from a Unix timestamp.
    ///
    /// Returns `None` if the specified timestamp cannot be represented.
    fn checked_from_timestamp(timestamp: u64) -> Option<SystemTime> {
        SystemTime::UNIX_EPOCH.checked_add(Duration::from_secs(timestamp))
    }

    /// Convert the system time into a timestamp in seconds from the Unix epoch.
    ///
    /// Returns an error if the system time is earlier the Unix epoch.
    fn as_timestamp(&self) -> Result<u64, SystemTimeError>;
}

impl SystemTimeExt for SystemTime {
    fn as_timestamp(&self) -> Result<u64, SystemTimeError> {
        Ok(self.duration_since(SystemTime::UNIX_EPOCH)?.as_secs())
    }
}
