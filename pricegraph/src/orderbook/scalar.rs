//! This module contains definitions for measurement scalars used by the
//! orderbook graph representation.

use crate::encoding;
use crate::FEE_FACTOR;
use std::cmp;
use std::ops;

/// An exchange price. Limit prices on the exchange are represented by a
/// fraction of two `u128`s representing a buy and sell amount. These limit
/// prices implicitly include fees, that is they must be respected **after**
/// fees are applied. As such, the actual exchange rate for a trade (the ratio
/// of executed buy amount over executed sell amount) is strictly greater than
/// and **never equal to** the limit price of an order.
///
/// Prices are guaranteed to be a strictly positive finite real numbers.
#[derive(Clone, Copy, Debug, PartialEq, PartialOrd)]
pub struct Price(f64);

impl Price {
    /// Creates a new price from a `f64` value. Returns `None` if the price
    /// value is not valid. Specifically, it must be in the range `(0, +∞)`.
    pub fn new(value: f64) -> Option<Self> {
        if is_strictly_positive_and_finite(value) {
            Some(Price(value))
        } else {
            None
        }
    }

    /// Creates a new price from an exchange price fraction.
    pub fn from_fraction(price: &encoding::Price) -> Option<Self> {
        if price.numerator != 0 && price.denominator != 0 {
            Some(Price(assert_strictly_positive_and_finite(
                price.numerator as f64 / price.denominator as f64,
            )))
        } else {
            None
        }
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

    /// Converts an exchange rate into a price with implicit fees.
    pub fn price(self) -> Price {
        Price(assert_strictly_positive_and_finite(self.0 / FEE_FACTOR))
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
    pub fn weight(self) -> f64 {
        self.0.log2()
    }
}

// NOTE: We can implement `Eq` for `ExchangeRate` since its value is garanteed
// to not be `NaN`.
impl Eq for ExchangeRate {}

impl Ord for ExchangeRate {
    fn cmp(&self, rhs: &Self) -> cmp::Ordering {
        self.partial_cmp(rhs).expect("exchange rate cannot be NaN")
    }
}

impl PartialEq<f64> for ExchangeRate {
    fn eq(&self, rhs: &f64) -> bool {
        self.0 == *rhs
    }
}

impl PartialOrd<f64> for ExchangeRate {
    fn partial_cmp(&self, rhs: &f64) -> Option<cmp::Ordering> {
        self.0.partial_cmp(rhs)
    }
}

macro_rules! impl_deref_f64 {
    ($($t:ty),*) => {$(
        impl ops::Deref for $t {
            type Target = f64;
            fn deref(&self) -> &Self::Target {
                &self.0
            }
        }
    )*}
}

impl_deref_f64!(Price, ExchangeRate);

macro_rules! impl_binop {
    ($(
        $op:tt => {
            $trait:ident :: $method:ident,
            $trait_assign:ident :: $method_assign:ident
        }
    )*) => {$(
        impl ops::$trait for ExchangeRate {
            type Output = ExchangeRate;

            fn $method(self, rhs: Self) -> Self::Output {
                ExchangeRate(assert_strictly_positive_and_finite(self.0 $op rhs.0))
            }
        }

        impl ops::$trait_assign for ExchangeRate {
            fn $method_assign(&mut self, rhs: Self) {
                self.0 = assert_strictly_positive_and_finite(self.0 $op rhs.0);
            }
        }
    )*}
}

impl_binop! {
    * => { Mul::mul, MulAssign::mul_assign }
}

fn is_strictly_positive_and_finite(value: f64) -> bool {
    value > 0.0 && value < f64::INFINITY
}

/// Internal method for asserting values are strictly positive and finite. This
/// is used in debug builds to ensure assumptions about price and exchange rate
/// operations hold. In release mode, this method does nothing.
fn assert_strictly_positive_and_finite(value: f64) -> f64 {
    debug_assert!(is_strictly_positive_and_finite(value));
    value
}
