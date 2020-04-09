//! Module implementing floating point arithmetic for the price graph.

use primitive_types::U256;
use std::cmp;
use std::f64;

/// Convert an unsigned 256-bit integer into a `f64`.
pub fn u256_to_f64(u: U256) -> f64 {
    let (u, factor) = match u {
        U256([_, _, 0, 0]) => (u, 1.0),
        U256([_, _, _, 0]) => (u >> 64, 2.0f64.powi(64)),
        U256([_, _, _, _]) => (u >> 128, 2.0f64.powi(128)),
    };
    (u.low_u128() as f64) * factor
}

/// Calculates the minimum of two floats. Note that we cannot use the standard
/// library `std::cmp::min` here since `f64` does not implement `Ord`. This be
/// because there is no real ordering for `NaN`s and `NaN < 0 == false` and
/// `NaN >= 0 == false` (cf. IEEE 754-2008 section 5.11).
///
/// # Panics
///
/// If any of the two floats are NaN.
pub fn min(a: f64, b: f64) -> f64 {
    match a
        .partial_cmp(&b)
        .expect("orderbooks cannot have NaN quantities")
    {
        cmp::Ordering::Less => a,
        _ => b,
    }
}
