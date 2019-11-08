use std::convert::TryInto;
use web3::types::U256;

/// Convert a u128 to a U256.
pub fn u128_to_u256(x: u128) -> U256 {
    U256::from_big_endian(&x.to_be_bytes())
}

/// Convert a U256 to a u128.
///
/// # Panics
///
/// Panics if the U256 overflows a u128.
pub fn u256_to_u128(x: U256) -> u128 {
    let mut bytes = [0u8; 32];
    x.to_big_endian(&mut bytes[..]);

    let hi = u128::from_be_bytes(bytes[..16].try_into().expect("correct slice length"));
    let lo = u128::from_be_bytes(bytes[16..].try_into().expect("correct slice length"));

    assert_eq!(hi, 0, "U256 to u128 overflow");
    lo
}

#[cfg(test)]
pub mod tests {
    use super::*;

    #[test]
    fn test_u128_to_u256() {
        assert_eq!(
            u128_to_u256(u128::max_value()),
            U256::from_dec_str("340282366920938463463374607431768211455").unwrap(),
            "failed on 128::max_value()"
        );
        assert_eq!(u128_to_u256(1u128), U256::from(1), "failed on 1u128");
        assert_eq!(u128_to_u256(0u128), U256::from(0), "failed on 0u128");
    }

    #[test]
    fn test_256_to_u128_works() {
        assert_eq!(0u128, u256_to_u128(U256::from(0)));
        assert_eq!(1u128, u256_to_u128(U256::from(1)));
        assert_eq!(
            u128::max_value(),
            u256_to_u128(U256::from_dec_str("340282366920938463463374607431768211455").unwrap())
        );
    }

    #[test]
    #[should_panic]
    fn test_u256_to_u128_panics_on_overflow() {
        u256_to_u128(U256::from_dec_str("340282366920938463463374607431768211456").unwrap());
    }
}
