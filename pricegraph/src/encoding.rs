//! This module implements decoding for the standard `BatchExchange` contract
//! encoded orders.

use primitive_types::{H160, U256};
use thiserror::Error;

/// The stride of an orderbook element in bytes.
pub const ELEMENT_STRIDE: usize = 112;

/// A type alias for a batch ID.
pub type BatchId = u32;

/// A type alias for a token ID.
pub type TokenId = u16;

/// A type alias for a user ID.
pub type UserId = H160;

/// A struct representing a buy/sell token pair.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct TokenPair {
    /// The buy token.
    pub buy: TokenId,
    /// The sell token.
    pub sell: TokenId,
}

/// A struct representing the validity of an order.
#[derive(Debug, PartialEq)]
pub struct Validity {
    /// The batch starting from which the order is valid.
    pub from: BatchId,
    /// The last batch the order is valid for.
    pub to: BatchId,
}

/// A price expressed as a fraction of buy and sell amounts.
#[derive(Debug, PartialEq)]
pub struct Price {
    /// The price numerator, or the buy amount.
    pub numerator: u128,
    /// The price denominator, or the sell amount.
    pub denominator: u128,
}

/// An orderbook element that is retrieved from the smart contract.
#[derive(Debug, PartialEq)]
pub struct Element {
    /// The user that placed the order.
    pub user: UserId,
    /// The user's sell token balance.
    pub balance: U256,
    /// The token pair for which this order was placed.
    pub pair: TokenPair,
    /// The validity of the order.
    pub valid: Validity,
    /// The price fraction for the order.
    pub price: Price,
    /// The remaining sell amount available to this order.
    pub remaining_sell_amount: u128,
}

impl Element {
    pub fn read_all<'a>(bytes: &'a [u8]) -> Result<impl Iterator<Item = Self> + 'a, InvalidLength> {
        if bytes.len() % ELEMENT_STRIDE != 0 {
            return Err(InvalidLength(bytes.len()));
        }

        Ok(bytes.chunks(ELEMENT_STRIDE).map(|mut chunk| {
            macro_rules! read {
                (u16) => {
                    u16::from_be_bytes(read!(2))
                };
                (u32) => {
                    u32::from_be_bytes(read!(4))
                };
                (u128) => {
                    u128::from_be_bytes(read!(16))
                };
                (U256) => {
                    U256::from_big_endian(&read!(32))
                };
                (H160) => {
                    H160(read!(20))
                };
                ($n:expr) => {{
                    let mut buf = [0u8; $n];
                    buf.copy_from_slice(&chunk[..$n]);
                    chunk = &chunk[$n..];
                    buf
                }};
            }

            #[allow(unused_assignments, clippy::eval_order_dependence)]
            Element {
                user: read!(H160),
                balance: read!(U256),
                pair: TokenPair {
                    buy: read!(u16),
                    sell: read!(u16),
                },
                valid: Validity {
                    from: read!(u32),
                    to: read!(u32),
                },
                price: Price {
                    numerator: read!(u128),
                    denominator: read!(u128),
                },
                remaining_sell_amount: read!(u128),
            }
        }))
    }
}

#[derive(Debug, Error)]
#[error("invalid encoded order elements byte length {0}")]
pub struct InvalidLength(usize);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[allow(clippy::unreadable_literal)]
    fn read_all_elements() {
        let bytes = (0u8..224).collect::<Vec<_>>();
        assert_eq!(
            Element::read_all(&bytes).unwrap().next(),
            Some(Element {
                user: H160(
                    *b"\x00\x01\x02\x03\x04\x05\x06\x07\x08\x09\
                       \x0a\x0b\x0c\x0d\x0e\x0f\x10\x11\x12\x13"
                ),
                balance: U256([
                    0x2c2d2e2f30313233,
                    0x2425262728292a2b,
                    0x1c1d1e1f20212223,
                    0x1415161718191a1b,
                ]),
                pair: TokenPair {
                    buy: 0x3435,
                    sell: 0x3637,
                },
                valid: Validity {
                    from: 0x38393a3b,
                    to: 0x3c3d3e3f,
                },
                price: Price {
                    numerator: 0x404142434445464748494a4b4c4d4e4f,
                    denominator: 0x505152535455565758595a5b5c5d5e5f,
                },
                remaining_sell_amount: 0x606162636465666768696a6b6c6d6e6f,
            })
        );
    }
}
