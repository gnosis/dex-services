//! Module containing `serde` serialization helpers.

use serde::{
    de::{Deserialize, Deserializer, Error},
    ser::{Serialize, Serializer},
};
use std::marker::PhantomData;
use typenum::Unsigned;

/// A format version identifier to prevent previous versions of event registry
/// from being parsed as if
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Version<T>(PhantomData<T>);

impl<T> Version<T>
where
    T: Unsigned,
{
    /// The version number value required for serializing and deserializing.
    pub const VALUE: u32 = T::U32;
}

impl<T> Serialize for Version<T>
where
    T: Unsigned,
{
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        u32::serialize(&Self::VALUE, serializer)
    }
}

impl<'de, T> Deserialize<'de> for Version<T>
where
    T: Unsigned,
{
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let version = u32::deserialize(deserializer)?;
        if Self::VALUE == version {
            Ok(Default::default())
        } else {
            Err(D::Error::custom(format!(
                "invalid version '{}', expected '{}'",
                version,
                Self::VALUE,
            )))
        }
    }
}
