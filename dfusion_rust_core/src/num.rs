use crate::util::*;
use std::fmt::{self, Debug, Display, Formatter};
use std::ops::Div;
use std::{i64, u64};
use web3::types::U256;

/// A partial implementation of a 256-bit signed integer needed for accumulating
/// token conservation values.
#[derive(Clone, Copy, Eq, PartialEq)]
pub struct I256(pub U256);

impl I256 {
    /// Creates a 0 valued I256.
    pub fn zero() -> I256 {
        I256(U256::zero())
    }

    /// Creates a I256 from a U256 and checks for overflow
    pub fn checked_from(from: U256) -> Option<I256> {
        let value = I256(from);
        if value.is_negative() {
            None
        } else {
            Some(value)
        }
    }

    /// Creates a U256 from a I256 if it is non-negative.
    pub fn checked_into(self) -> Option<U256> {
        if self.is_negative() {
            None
        } else {
            Some(self.0)
        }
    }

    /// Returns the smallest value that can be represented by this integer type.
    pub fn min_value() -> I256 {
        I256(U256([u64::MIN, u64::MIN, u64::MIN, i64::MIN as _]))
    }

    /// Returns the largest value that can be represented by this integer type.
    #[allow(dead_code)]
    pub fn max_value() -> I256 {
        I256(U256([u64::MAX, u64::MAX, u64::MAX, i64::MAX as _]))
    }

    /// Checked integer addition. Computes `self + rhs`, returning `None` if
    /// overflow occurred.
    pub fn checked_add(self, rhs: I256) -> Option<I256> {
        // can't just compute the `self - (-rhs)` because we might get some
        // false-positive overflows when `rhs == I256::min_value()`

        let (sub, _) = self.0.overflowing_add(rhs.0);
        let result = I256(sub);

        // check for overflow
        match (self.signum64(), rhs.signum64(), result.signum64()) {
            (1, 1, -1) => None,
            (-1, -1, 1) | (-1, -1, 0) => None,
            _ => Some(result),
        }
    }

    /// Checked integer subtraction. Computes `self - rhs`, returning `None` if
    /// overflow occurred.
    pub fn checked_sub(self, rhs: I256) -> Option<I256> {
        // can't just compute the `self + (-rhs)` because we might get some
        // false-positive overflows when `rhs == I256::min_value()`

        let (sub, _) = self.0.overflowing_sub(rhs.0);
        let result = I256(sub);

        // check for overflow
        match (self.signum64(), rhs.signum64(), result.signum64()) {
            (1, -1, -1) => None,
            (-1, 1, 1) => None,
            _ => Some(result),
        }
    }

    /// Checked negation. Computes `-self`, returning None if `self == MIN`.
    pub fn checked_neg(self) -> Option<I256> {
        if self != I256::min_value() {
            let (twos_complement, _) = (self.0 ^ U256::max_value()).overflowing_add(U256::one());
            Some(I256(twos_complement))
        } else {
            None
        }
    }

    /// Returns `true` if `self` is positive and `false` if the number is zero or
    /// negative.
    pub fn is_positive(self) -> bool {
        self.signum64().is_positive()
    }

    /// Returns `true` if `self` is negative and `false` if the number is zero or
    /// positive.
    pub fn is_negative(self) -> bool {
        self.signum64().is_negative()
    }

    /// Returns an `i64` representing the sign of the number.
    fn signum64(self) -> i64 {
        let most_significant_word = (self.0).0[3] as i64;
        if most_significant_word.is_negative() {
            -1
        } else if self == I256::zero() {
            0
        } else {
            1
        }
    }

    /// Returns the absolute value of `self` as a `U256`.
    fn abs(self) -> U256 {
        if self.is_negative() {
            twos_complement(self.0)
        } else {
            self.0
        }
    }
}

impl Div<i32> for I256 {
    type Output = Self;

    fn div(self, rhs: i32) -> Self::Output {
        let result = self.abs() / rhs.abs();
        if self.signum64() * i64::from(rhs.signum()) == -1 {
            // don't use checked_neg here because that will panic when:
            // `I256::min_value() / 1` which should be valid
            I256(twos_complement(result))
        } else {
            I256(result)
        }
    }
}

pub trait U256CheckedAddI256Ext {
    fn checked_add_i256(self, rhs: I256) -> Option<U256>;
}

impl U256CheckedAddI256Ext for U256 {
    fn checked_add_i256(self, rhs: I256) -> Option<U256> {
        if rhs.is_negative() {
            self.checked_sub(twos_complement(rhs.0))
        } else {
            self.checked_add(rhs.0)
        }
    }
}

impl From<U256> for I256 {
    fn from(from: U256) -> I256 {
        I256(from)
    }
}

impl From<i128> for I256 {
    fn from(from: i128) -> I256 {
        if from.is_negative() {
            let abs = from.wrapping_neg() as u128;
            I256::from(abs).checked_neg().expect("no overflow")
        } else {
            #[allow(clippy::cast_lossless)]
            I256::from(from as u128)
        }
    }
}

impl From<u128> for I256 {
    fn from(from: u128) -> I256 {
        I256(u128_to_u256(from))
    }
}

impl From<i32> for I256 {
    fn from(from: i32) -> I256 {
        I256::from(i128::from(from))
    }
}

impl Into<U256> for I256 {
    fn into(self) -> U256 {
        self.0
    }
}

impl Debug for I256 {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        let sign = if self.is_negative() {
            "-"
        } else if f.sign_plus() {
            "+"
        } else {
            ""
        };
        let abs = self.abs();

        f.write_str(sign)?;
        Debug::fmt(&abs, f)
    }
}

impl Display for I256 {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        Debug::fmt(self, f)
    }
}

/// Compute the two's complement of a U256.
fn twos_complement(u: U256) -> U256 {
    let (twos_complement, _) = (u ^ U256::max_value()).overflowing_add(U256::one());
    twos_complement
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_checked_from() {
        assert_eq!(I256::checked_from(U256::from(10)), Some(I256::from(10)));
        assert_eq!(I256::checked_from(U256::max_value()), None);
    }

    #[test]
    fn test_checked_into() {
        assert_eq!(I256::from(1).checked_into(), Some(U256::one()));
        assert_eq!(
            I256::max_value().checked_into(),
            Some(U256::max_value() / 2)
        );
        assert_eq!(I256::zero().checked_into(), Some(U256::zero()));
        assert_eq!(I256::from(-1).checked_into(), None);
        assert_eq!(I256::min_value().checked_into(), None);
    }

    #[test]
    fn test_min_max_values() {
        // min is 0x800...0 and max is 0x7ff...f
        assert_eq!(I256::min_value(), I256(U256::one() << 255));
        assert_eq!(I256::max_value(), I256((U256::one() << 255) - U256::one()));
    }

    #[test]
    fn test_add() {
        assert_eq!(
            I256::max_value().checked_add(I256::min_value()).unwrap(),
            I256::from(-1)
        );
        assert_eq!(
            I256::from(1).checked_add(I256::from(1)).unwrap(),
            I256::from(2)
        );
        assert_eq!(
            I256::from(1).checked_add(I256::from(-1)).unwrap(),
            I256::zero()
        );
        assert_eq!(
            I256::from(-1).checked_add(I256::from(2)).unwrap(),
            I256::from(1)
        );
        assert_eq!(
            I256::from(1).checked_add(I256::from(-2)).unwrap(),
            I256::from(-1)
        );
    }

    #[test]
    fn test_add_overflow() {
        assert_eq!(I256::max_value().checked_add(I256::from(1)), None);
        assert_eq!(I256::min_value().checked_add(I256::from(-1)), None);
        assert_eq!(I256::max_value().checked_add(I256::max_value()), None);
        assert_eq!(I256::min_value().checked_add(I256::min_value()), None);
    }

    #[test]
    fn test_sub() {
        assert_eq!(
            I256::min_value().checked_sub(I256::from(-1)).unwrap(),
            I256::max_value().checked_neg().unwrap(),
        );
        assert_eq!(
            I256::from(1).checked_sub(I256::from(1)).unwrap(),
            I256::zero()
        );
        assert_eq!(
            I256::from(-1).checked_sub(I256::from(-1)).unwrap(),
            I256::zero()
        );
        assert_eq!(
            I256::from(1).checked_sub(I256::from(-1)).unwrap(),
            I256::from(2)
        );
        assert_eq!(
            I256::from(-1).checked_sub(I256::from(1)).unwrap(),
            I256::from(-2)
        );
    }

    #[test]
    fn test_sub_overflow() {
        assert_eq!(I256::min_value().checked_sub(I256::from(1)), None);
        assert_eq!(I256::max_value().checked_sub(I256::from(-1)), None);
        assert_eq!(I256::max_value().checked_sub(I256::min_value()), None);
        assert_eq!(I256::min_value().checked_sub(I256::max_value()), None);
    }

    #[test]
    fn test_neg() {
        assert_eq!(I256::from(1).checked_neg().unwrap(), I256::from(-1));
        assert_eq!(I256::min_value().checked_neg(), None);
    }

    #[test]
    fn test_is_positive() {
        assert_eq!(I256::from(1).is_positive(), true);
        assert_eq!(I256::max_value().is_positive(), true);
        assert_eq!(I256::from(-1).is_positive(), false);
        assert_eq!(I256::min_value().is_positive(), false);
        assert_eq!(I256::zero().is_positive(), false);
    }

    #[test]
    fn test_is_negative() {
        assert_eq!(I256::from(1).is_negative(), false);
        assert_eq!(I256::max_value().is_negative(), false);
        assert_eq!(I256::from(-1).is_negative(), true);
        assert_eq!(I256::min_value().is_negative(), true);
        assert_eq!(I256::zero().is_negative(), false);
    }

    #[test]
    fn test_signum() {
        assert_eq!(I256::from(1).signum64(), 1);
        assert_eq!(I256::max_value().signum64(), 1);
        assert_eq!(I256::from(-1).signum64(), -1);
        assert_eq!(I256::min_value().signum64(), -1);
        assert_eq!(I256::zero().signum64(), 0);
    }

    #[test]
    fn test_div() {
        assert_eq!(I256::zero() / 1, I256::zero());
        assert_eq!(I256::zero() / -1, I256::zero());

        assert_eq!(I256::min_value() / 1, I256::min_value());
        assert_eq!(I256::from(1) / -1, I256::from(-1));
        assert_eq!(I256::from(-1) / -1, I256::from(1));

        assert_eq!(I256::from(10) / 2, I256::from(5));
    }

    #[test]
    fn test_u256_checked_add_i256() {
        assert_eq!(
            U256::from(10).checked_add_i256(I256::from(-1)).unwrap(),
            U256::from(9)
        );
        assert_eq!(
            U256::from(10).checked_add_i256(I256::from(1)).unwrap(),
            U256::from(11)
        );
    }

    #[test]
    fn test_u256_checked_add_i256_overflow() {
        assert_eq!(U256::max_value().checked_add_i256(I256::from(1)), None);
        assert_eq!(U256::zero().checked_add_i256(I256::from(-1)), None);
    }

    #[test]
    fn test_i256_conversions() {
        assert_eq!(I256::from(-1), I256::from(1).checked_neg().unwrap());
        assert_eq!(
            I256::from(i128::min_value()),
            I256::from(1u128 << 127).checked_neg().unwrap(),
        );
    }

    #[test]
    fn test_twos_complement() {
        assert_eq!(twos_complement(U256::zero()), U256::zero());

        assert_eq!(twos_complement(U256::one()), U256::max_value());
        assert_eq!(twos_complement(U256::max_value()), U256::one());

        assert_eq!(twos_complement(U256::one() << 255), U256::one() << 255);

        assert_eq!(twos_complement(2.into()), U256::max_value() - U256::one());
        assert_eq!(twos_complement(U256::max_value() - U256::one()), 2.into());
    }
}
