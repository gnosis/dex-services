//! This module contains definitions for measurement scalars used by the
//! orderbook graph representation.

use crate::{encoding::PriceFraction, num, FEE_FACTOR};
use petgraph::algo::FloatMeasure;
use std::{cmp, fmt, ops};

/// An exchange limit price. Limit prices on the exchange are represented by a
/// fraction of two `u128`s representing a buy and sell amount. These limit
/// prices implicitly include fees, that is they must be respected **after**
/// fees are applied. As such, the actual exchange rate for a trade (the ratio
/// of executed buy amount over executed sell amount) is strictly greater than
/// and **never equal to** the limit price of an order.
///
/// Prices are guaranteed to be a strictly positive finite real numbers.
#[derive(Clone, Copy, Debug, PartialEq, PartialOrd)]
pub struct LimitPrice(f64);

impl LimitPrice {
    /// Creates a new price from a `f64` value. Returns `None` if the price
    /// value is not valid. Specifically, it must be in the range `(0, +∞)`.
    pub fn new(value: f64) -> Option<Self> {
        if num::is_strictly_positive_and_finite(value) {
            Some(LimitPrice(value))
        } else {
            None
        }
    }

    /// Creates a new price from a `f64` value.
    ///
    /// # Panics
    ///
    /// Panics if the value is not valid.
    pub fn from_raw(value: f64) -> Self {
        Self::new(value).expect("invalid price value")
    }

    /// Creates a new price from an exchange price fraction.
    pub fn from_fraction(price: &PriceFraction) -> Option<Self> {
        if price.numerator != 0 && price.denominator != 0 {
            Some(LimitPrice(assert_strictly_positive_and_finite(
                price.numerator as f64 / price.denominator as f64,
            )))
        } else {
            None
        }
    }

    /// Gets the value as a `f64`.
    pub fn value(self) -> f64 {
        self.0
    }

    /// Converts a price into an effective exchange rate with explicit fees.
    pub fn exchange_rate(self) -> ExchangeRate {
        ExchangeRate(assert_strictly_positive_and_finite(self.0 * FEE_FACTOR))
    }
}

/// An effective exchange rate that explicitly includes fees. As such, the
/// actual exchange rate for a trade (the ratio of executed buy amount over
/// executed sell amount) is greater than **or equal to** the limit exchange
/// rate for an order.
///
/// Exchange rates are guaranteed to be a strictly positive finite real numbers.
#[derive(Clone, Copy, Debug, PartialEq, PartialOrd)]
pub struct ExchangeRate(f64);

impl ExchangeRate {
    /// The 1:1 exchange rate.
    pub const IDENTITY: ExchangeRate = ExchangeRate(1.0);

    /// Creates a new exchange rate from a `f64` value. Returns `None` if the
    /// exchange rate value is not valid. Specifically, it must be in the range
    /// `(0, +∞)`.
    pub fn new(value: f64) -> Option<Self> {
        if num::is_strictly_positive_and_finite(value) {
            Some(ExchangeRate(value))
        } else {
            None
        }
    }

    /// Gets the value as a `f64`.
    pub fn value(self) -> f64 {
        self.0
    }

    /// Converts an exchange rate into a price with implicit fees.
    pub fn price(self) -> LimitPrice {
        LimitPrice(assert_strictly_positive_and_finite(self.0 / FEE_FACTOR))
    }

    /// Computes the inverse exchange rate.
    pub fn inverse(self) -> Self {
        ExchangeRate(assert_strictly_positive_and_finite(1.0 / self.0))
    }

    /// Computes the edge weight for an exchange rate for the orderbook
    /// projection graph.
    ///
    /// This is the base-2 logarithm of the exchange rate. This eanbles path
    /// weights to be computed using addition instead of multiplication.
    pub fn weight(self) -> Weight {
        Weight::new(self.0)
    }
}

macro_rules! impl_cmp {
    ($($t:ty),*) => {$(
        impl Eq for $t {}

        impl Ord for $t {
            fn cmp(&self, rhs: &Self) -> cmp::Ordering {
                self.partial_cmp(rhs).expect("exchange rate cannot be NaN")
            }
        }

        impl PartialEq<f64> for $t {
            fn eq(&self, rhs: &f64) -> bool {
                self.0 == *rhs
            }
        }

        impl PartialOrd<f64> for $t {
            fn partial_cmp(&self, rhs: &f64) -> Option<cmp::Ordering> {
                self.0.partial_cmp(rhs)
            }
        }
    )*};
}

impl_cmp! { LimitPrice, ExchangeRate }

macro_rules! impl_binop {
    ($(
        $op:tt for $t:ty => {
            $trait:ident :: $method:ident,
            $trait_assign:ident :: $method_assign:ident
        }
    )*) => {$(
        impl ops::$trait for $t {
            type Output = $t;

            fn $method(self, rhs: Self) -> Self::Output {
                Self(assert_strictly_positive_and_finite(self.0 $op rhs.0))
            }
        }

        impl ops::$trait_assign for $t {
            fn $method_assign(&mut self, rhs: Self) {
                self.0 = assert_strictly_positive_and_finite(self.0 $op rhs.0);
            }
        }
    )*}
}

impl_binop! {
    * for ExchangeRate => { Mul::mul, MulAssign::mul_assign }
}

/// Internal method for asserting values are strictly positive and finite. This
/// is used in debug builds to ensure assumptions about price and exchange rate
/// operations hold. In release mode, this method does nothing.
fn assert_strictly_positive_and_finite(value: f64) -> f64 {
    debug_assert!(num::is_strictly_positive_and_finite(value));
    value
}

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
/// to be able to represent the two's complement of 7517784 (for the --7517784)
/// while still reserving a special value for ∞.to have a special "infinite"
/// value which is required by the Bellman-Ford implementation.
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
        // that the the range of non-infinite values are semetric around `0`.
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
