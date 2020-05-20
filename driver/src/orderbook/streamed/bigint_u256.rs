use ethcontract::U256;
use num::{bigint::Sign, BigInt, BigUint};

/// None if U256 cannot represent the number.
pub fn bigint_to_u256(n: &BigInt) -> Option<U256> {
    match n.to_bytes_le() {
        (Sign::NoSign, _) => Some(U256::zero()),
        (Sign::Plus, bytes) if bytes.len() <= 256 / 8 => Some(U256::from_little_endian(&bytes)),
        _ => None,
    }
}

pub fn u256_to_biguint(n: U256) -> BigUint {
    let mut bytes = [0u8; 256 / 8];
    n.to_little_endian(&mut bytes);
    BigUint::from_bytes_le(&bytes)
}

pub fn u256_to_bigint(n: U256) -> BigInt {
    BigInt::from_biguint(Sign::Plus, u256_to_biguint(n))
}

#[cfg(test)]
mod tests {
    use super::*;
    use num::pow::Pow;

    #[test]
    fn zero() {
        assert_eq!(bigint_to_u256(&BigInt::from(0)), Some(U256::from(0)));
        assert_eq!(u256_to_bigint(U256::from(0)), BigInt::from(0));
    }

    #[test]
    fn negative() {
        assert_eq!(bigint_to_u256(&BigInt::from(-1)), None);
    }

    #[test]
    fn positive() {
        assert_eq!(bigint_to_u256(&BigInt::from(1)), Some(U256::from(1)));
        assert_eq!(u256_to_bigint(U256::from(1)), BigInt::from(1));
    }

    #[test]
    fn large() {
        let bigint = BigInt::from(2).pow(256u32) - BigInt::from(1);
        let u256 = U256::MAX;
        assert_eq!(bigint_to_u256(&bigint), Some(u256));
        assert_eq!(u256_to_bigint(u256), bigint);
    }

    #[test]
    fn too_large() {
        assert_eq!(bigint_to_u256(&BigInt::from(2).pow(256u32)), None);
    }
}
