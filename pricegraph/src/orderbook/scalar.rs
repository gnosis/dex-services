//! This module contains definitions for measurement scalars used by the
//! orderbook graph representation.

use crate::{encoding::PriceFraction, num, orderbook::weight::Weight, FEE_FACTOR};
use std::cmp;

/// An exchange limit price. Limit prices on the exchange are represented by a
/// fraction of two `u128`s representing a buy and sell amount. These limit
/// prices implicitly include fees, that is they must be respected **after**
/// fees are applied. As such, the actual exchange rate for a trade (the ratio
/// of executed buy amount over executed sell amount) is strictly greater than
/// and **never equal to** the limit price of an order.
///
/// Prices are guaranteed to be a strictly positive finite real numbers.
#[derive(Clone, Copy, Debug, PartialEq)]
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
#[derive(Clone, Copy, Debug, PartialEq)]
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

    pub fn checked_mul(self, rhs: Self) -> Option<Self> {
        let result = self.0 * rhs.0;
        if num::is_strictly_positive_and_finite(result) {
            Some(Self(result))
        } else {
            None
        }
    }
}

macro_rules! impl_cmp {
    ($($t:ty),*) => {$(
        impl Eq for $t {}

        impl PartialOrd for $t {
            fn partial_cmp(&self, rhs: &Self) -> Option<cmp::Ordering> {
                self.0.partial_cmp(&rhs.0)
            }
        }

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

/// Internal method for asserting values are strictly positive and finite. This
/// is used in debug builds to ensure assumptions about price and exchange rate
/// operations hold. In release mode, this method does nothing.
fn assert_strictly_positive_and_finite(value: f64) -> f64 {
    debug_assert!(num::is_strictly_positive_and_finite(value));
    value
}
