//! Module contains implementation of the weight used by order in graph path
//! finding algorightms.

use petgraph::algo::FloatMeasure;
use std::{cmp, fmt, ops};

/// A signed fixed point number with 24 magnitude bits and 104 fractional bits.
///
/// Note the that size of the magnitude and fractional components was carefully
/// chosen for exchange rate weights. Specifically, weights must be in a
/// logarithmic scale so that adding them is equivalent to multiplying exchange
/// rates. Since `log2` of the exchange rate is used, and the range of these
/// exchange rates are:
/// ```text
/// [MIN_AMOUNT / u128::MAX, u128::MAX / MIN_AMOUNT]
/// ```
///
/// In the logarithmic scale, this range is:
/// ```text
/// [-114.71, 114.71]
/// ```
///
/// Furthermore, these weights can be added at most 2^16 times (this is the
/// maximum number of tokens in the exchange), so the total range that must be
/// representable by the magnitude bits is:
/// ```text
/// [-7517784, 7517784]
/// ```
///
/// This number fits in 23 bits. However, an additional bit is needed in order
/// to be able to represent the two's complement of 7517784 (for the -7517784)
/// while still reserving a special value for ∞ which is required by the
/// Bellman-Ford implementation.
///
/// This leaves 104 fractional bits. Note that we want **as many fractional bits
/// as possible** to keep as much precision as possible for values very close to
/// `0.0`. With `104` fractional bits, we can represent without precision loss
/// any `f64` that is greater than `1e-51` (since `-104 - 53 == 51` where `53 is
/// the number of bits of precision in an `f64`.
type Fixed24x104 = i128;

/// The 24x104 fixed point number scaling factor.
///
/// Note that this value is a **power of 2** to make sure that multiplying with
/// `f64`s does not cause precision issues.
const FIXED_24X104_SCALING_FACTOR: f64 = (1u128 << 104) as _;

/// An opaque weight for an exchange rate used by the pathfinding algorithm.
///
/// Internally, the weight is a represented as an fixed point number.
#[derive(Clone, Copy, Default, Eq, PartialEq)]
pub struct Weight(Fixed24x104);

impl Weight {
    /// Creates a new graph weight from a floating point number.
    pub fn new(value: f64) -> Self {
        // TODO(nlordell): In the future, it might be nice to compute the `log2`
        // already as a fixed point value in order to have more precision.
        // Currently, since it is computed as a `f64` it is limited to 53 bits
        // of precision instead of the full `104 + 7` that is possible given
        // the range of the `log2` values and size of the fixed point number.

        let weight = value.log2() * FIXED_24X104_SCALING_FACTOR;
        debug_assert!(
            -114.72 * FIXED_24X104_SCALING_FACTOR <= weight
                && weight < 114.72 * FIXED_24X104_SCALING_FACTOR,
        );

        Weight(weight as _)
    }
}

impl FloatMeasure for Weight {
    fn zero() -> Self {
        Weight(0)
    }

    fn infinite() -> Self {
        // NOTE: Use a special marker value to represent +∞ which is needed by
        // the `petgraph` Bellman-Ford implementation. `i128::MIN` is chosen so
        // that the the range of non-infinite values are symmetric around `0`.
        Weight(i128::MIN)
    }
}

impl ops::Add for Weight {
    type Output = Self;

    fn add(self, rhs: Self) -> Self {
        // NOTE: The Bellman-Ford implementation relies on special behaviour
        // for +∞ such that: `+∞ * x == +∞`.
        if self == Weight::infinite() || rhs == Weight::infinite() {
            Weight::infinite()
        } else {
            Weight(self.0 + rhs.0)
        }
    }
}

impl cmp::PartialOrd for Weight {
    fn partial_cmp(&self, other: &Self) -> Option<cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl cmp::Ord for Weight {
    fn cmp(&self, other: &Self) -> cmp::Ordering {
        match (*self == Weight::infinite(), *other == Weight::infinite()) {
            (true, true) => cmp::Ordering::Equal,
            (true, false) => cmp::Ordering::Greater,
            (false, true) => cmp::Ordering::Less,
            _ => self.0.cmp(&other.0),
        }
    }
}

impl fmt::Debug for Weight {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let (value, xrate): (&dyn fmt::Debug, _) = if *self == Weight::infinite() {
            (&f64::INFINITY, f64::INFINITY)
        } else {
            let xrate = 2.0f64.powf((self.0 as f64) / FIXED_24X104_SCALING_FACTOR);
            (&self.0, xrate)
        };

        f.debug_struct("Weight")
            .field("value", value)
            .field("exchange_rate", &xrate)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{num, MIN_AMOUNT};

    #[test]
    fn weight_range_fits_in_fixed_point_number() {
        // NOTE: This test relies on float to integer conversion being
        // saturating, so just verify for sanity's sake:
        assert_eq!(f64::MAX as i128, i128::MAX);
        assert_eq!(-f64::MAX as i128, i128::MIN);

        const MAX_TOKENS: f64 = (1 << 16) as _;
        let max_xrate = 2.0f64.powi(128) / MIN_AMOUNT;

        let max_total_weight = {
            let weight = max_xrate.log2() * MAX_TOKENS * FIXED_24X104_SCALING_FACTOR;
            (weight + num::max_rounding_error(weight)) as Fixed24x104
        };
        let min_total_weight = -max_total_weight;

        // NOTE: The actual minimum value is reserved to represent +∞.
        assert!(min_total_weight > Fixed24x104::MIN + 1);
        assert!(max_total_weight < Fixed24x104::MAX);
    }

    #[test]
    fn weight_implements_ord() {
        assert_eq!(
            Weight::infinite().cmp(&Weight::infinite()),
            cmp::Ordering::Equal,
        );
        assert!(Weight::infinite() > Weight::new(1000.0));
        assert!(Weight::new(1000.0) < Weight::infinite());

        assert!(Weight::new(42.0) > Weight::new(1337.0f64.recip()));
    }

    #[test]
    fn weight_debug_displays_xrate() {
        assert_eq!(
            format!("{:?}", Weight::new(4.0)),
            format!(
                "Weight {{ value: {}, exchange_rate: {:?} }}",
                2i128 << 104,
                4.0,
            ),
        );
        assert_eq!(
            format!("{:?}", Weight::infinite()),
            format!(
                "Weight {{ value: {:?}, exchange_rate: {:?} }}",
                f64::INFINITY,
                f64::INFINITY,
            ),
        );
    }
}
