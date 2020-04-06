//! This module implements decoding for the standard `BatchExchange` contract
//! encoded orders.

use crate::{TokenId, UserId};
use primitive_types::U256;

pub const ELEMENT_STRIDE: usize = 112;

pub type BatchId = u32;

#[cfg_attr(test, derive(Debug, PartialEq))]
pub struct Element {
    pub user: UserId,
    pub balance: U256,
    pub pair: (TokenId, TokenId),
    pub valid: (BatchId, BatchId),
    pub price: (u128, u128),
    pub amount: u128,
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
                ($n:expr) => {{
                    let mut buf = [0u8; $n];
                    buf.copy_from_slice(&chunk[..$n]);
                    chunk = &chunk[$n..];
                    buf
                }};
            }

            #[allow(unused_assignments, clippy::eval_order_dependence)]
            Element {
                user: read!(20),
                balance: read!(U256),
                pair: (read!(u16), read!(u16)),
                valid: (read!(u32), read!(u32)),
                price: (read!(u128), read!(u128)),
                amount: read!(u128),
            }
        }))
    }
}

#[derive(Debug)]
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
                user: *b"\x00\x01\x02\x03\x04\x05\x06\x07\x08\x09\
                         \x0a\x0b\x0c\x0d\x0e\x0f\x10\x11\x12\x13",
                balance: U256([
                    0x2c2d2e2f30313233,
                    0x2425262728292a2b,
                    0x1c1d1e1f20212223,
                    0x1415161718191a1b,
                ]),
                pair: (0x3435, 0x3637),
                valid: (0x38393a3b, 0x3c3d3e3f),
                price: (
                    0x404142434445464748494a4b4c4d4e4f,
                    0x505152535455565758595a5b5c5d5e5f,
                ),
                amount: 0x606162636465666768696a6b6c6d6e6f,
            })
        );
    }
}
