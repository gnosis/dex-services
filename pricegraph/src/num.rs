//! Module implementing floating point arithmetic for the price graph.

use crate::MIN_AMOUNT;
use primitive_types::U256;
use std::cmp;
use std::f64;

/// The maximum rounding error for the specified amount, used for asserting that
/// amounts and balances remain coherent for tests and for `debug` profile.
///
/// The maximum rouding error is calcuated by finding the value of the least
/// significant digit of quantity. This means that quantities can only be off
/// by that least significant digit.
///
/// Another way of describing this is to compute an `f64::EPSILON` (which is for
/// `1.0`) equivalent for `quantity`. This implies
/// `max_rounding_error(1.0) == f64::EPSILON`.
pub fn max_rounding_error(quantity: f64) -> f64 {
    // NOTE: For discussion on the derivation of this formula, see:
    // https://github.com/gnosis/dex-services/pull/1012#discussion_r440627156
    const SIGN_EXPONENT_MASK: u64 = 0xfff0_0000_0000_0000;
    f64::from_bits(quantity.to_bits() & SIGN_EXPONENT_MASK) * f64::EPSILON
}

/// The maximum rouding error with an epsilon. This is because the assertion
/// `assert_approx_eq` uses `>` and `<` semantics, while the maximum rounding
/// error expects `>=` and `<=` semantics.
///
/// This method computes the next representable `f64` that is greater than
/// the maximum rounding error for `quantity`.
#[cfg(test)]
pub fn max_rounding_error_with_epsilon(quantity: f64) -> f64 {
    let r = max_rounding_error(quantity);
    r + max_rounding_error(r)
}

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

/// Compare two floats and returns an `Ordering`. This helper is used because
/// floats do not implement `Ord` because of `NaN` comparison semantics.
///
/// # Panics
///
/// If any of the two floats are NaN.
pub fn compare(a: f64, b: f64) -> cmp::Ordering {
    a.partial_cmp(&b)
        .expect("orderbooks cannot have NaN quantities")
}

/// Returns true if the specified number is within the range `(0.0, +Inf)`.
pub fn is_strictly_positive_and_finite(value: f64) -> bool {
    value > 0.0 && value < f64::INFINITY
}

/// Returns true if an amount is considered a dust amount. See `MIN_AMOUNT`
/// documentation for more details.
pub fn is_dust_amount(amount: f64) -> bool {
    amount < MIN_AMOUNT
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rounding_error_is_least_significant_digit() {
        fn is_mantissa_1(f: f64) -> bool {
            // NOTE: All mantissa bits are 0 when mantissa is 1.0.
            f.to_bits() << 12 == 0
        }

        for value in &[
            1.0f64,
            42.42,
            83_798_276_971_421_254_262_445_676_335_662_107_162.0,
            #[allow(clippy::inconsistent_digit_grouping)]
            f64::from_bits(0b0_10101010101_1111111111111111111111111111111111111111111111111111),
        ] {
            let upper_bound = value + max_rounding_error(*value);
            assert_eq!(upper_bound.to_bits() - value.to_bits(), 1);

            let lower_bound = value - max_rounding_error(*value);
            assert_eq!(
                value.to_bits() - lower_bound.to_bits(),
                if is_mantissa_1(*value) { 2 } else { 1 },
            );
        }
    }

    #[test]
    #[allow(clippy::float_cmp)]
    fn rounding_error_is_epsilon_for_1() {
        assert_eq!(max_rounding_error(1.0), f64::EPSILON);
    }

    #[test]
    fn strictly_positive_and_finite_numbers() {
        assert!(is_strictly_positive_and_finite(f64::EPSILON));
        assert!(is_strictly_positive_and_finite(42.0));

        assert!(!is_strictly_positive_and_finite(0.0));
        assert!(!is_strictly_positive_and_finite(f64::NAN));
        assert!(!is_strictly_positive_and_finite(f64::INFINITY));
        assert!(!is_strictly_positive_and_finite(f64::NEG_INFINITY));
        assert!(!is_strictly_positive_and_finite(-1.0));
    }
}
