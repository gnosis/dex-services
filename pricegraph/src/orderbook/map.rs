//! Module containing definition for `Map` type used for order and user lookup
//! in the orderbook.

pub use std::collections::hash_map::Entry;
use std::collections::HashMap;
#[cfg(any(feature = "bench", test))]
use std::{collections::hash_map::DefaultHasher, hash::BuildHasherDefault};

/// The map type used internally to look up users and orders in the orderbook.
#[cfg(not(any(feature = "bench", test)))]
pub type Map<K, V> = HashMap<K, V>;

/// The map type used internally to look up users and orders in the orderbook.
///
/// Note that for testing and benchmarking, the hash map uses a default state
/// instead of a random one in order for unit tests to not produce semi-random
/// results and for benchmarks to be more consistent.
#[cfg(any(feature = "bench", test))]
pub type Map<K, V> = HashMap<K, V, BuildHasherDefault<DefaultHasher>>;
