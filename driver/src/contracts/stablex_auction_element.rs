use crate::models::Order;
use byteorder::{BigEndian, ByteOrder};
use ethcontract::{Address, U256};

use crate::util::CeiledDiv;

pub const AUCTION_ELEMENT_WIDTH: usize = 112;
/// Indexed auction elements have the orderId appended at the end
pub const INDEXED_AUCTION_ELEMENT_WIDTH: usize = AUCTION_ELEMENT_WIDTH + 2;

#[derive(Debug, PartialEq)]
pub struct StableXAuctionElement {
    valid_from: U256,
    valid_until: U256,
    pub sell_token_balance: u128,
    pub order: Order,
}

impl StableXAuctionElement {
    pub fn in_auction(&self, index: U256) -> bool {
        self.valid_from <= index && index <= self.valid_until
    }

    /// Deserialize an auction element that has been serialized by the smart
    /// contract's `encodeAuctionElement` function.
    /// Sets `id` to `0` because this information is not contained in the
    /// serialized information.
    pub fn from_bytes(bytes: &[u8; AUCTION_ELEMENT_WIDTH]) -> Self {
        let mut indexed_bytes = [0u8; INDEXED_AUCTION_ELEMENT_WIDTH];
        for (index, bit) in bytes.iter().enumerate() {
            indexed_bytes[index] = *bit;
        }
        Self::from_indexed_bytes(&indexed_bytes)
    }

    /// Deserialize an auction element that has been serialized by the smart
    /// contract's `getFilteredOrdersPaginated` function.
    pub fn from_indexed_bytes(bytes: &[u8; INDEXED_AUCTION_ELEMENT_WIDTH]) -> Self {
        let account_id = Address::from_slice(&bytes[0..20]);

        // these go together (since sell_token_balance is emitted as u256 and treated as u128
        let sell_token_balance = BigEndian::read_u128(&bytes[36..52]);
        let sell_token_balance_padding = BigEndian::read_u128(&bytes[20..36]);
        assert_eq!(
            sell_token_balance_padding, 0,
            "User has too large balance to handle."
        );

        let buy_token = u16::from_le_bytes([bytes[53], bytes[52]]);
        let sell_token = u16::from_le_bytes([bytes[55], bytes[54]]);
        let valid_from = U256::from(u32::from_le_bytes([
            bytes[59], bytes[58], bytes[57], bytes[56],
        ]));
        let valid_until = U256::from(u32::from_le_bytes([
            bytes[63], bytes[62], bytes[61], bytes[60],
        ]));
        let numerator = BigEndian::read_u128(&bytes[64..80]);
        let denominator = BigEndian::read_u128(&bytes[80..96]);
        let remaining = BigEndian::read_u128(&bytes[96..112]);
        let id = u16::from_le_bytes([bytes[113], bytes[112]]);
        let (buy_amount, sell_amount) = compute_buy_sell_amounts(numerator, denominator, remaining);
        StableXAuctionElement {
            valid_from,
            valid_until,
            sell_token_balance,
            order: Order {
                id,
                account_id,
                buy_token,
                sell_token,
                buy_amount,
                sell_amount,
            },
        }
    }
}

fn compute_buy_sell_amounts(numerator: u128, denominator: u128, remaining: u128) -> (u128, u128) {
    assert!(
        remaining <= denominator,
        "Smart contract should never allow this inequality"
    );
    // 0 on sellAmount (remaining <= denominator) is nonsense, but solver can handle it.
    // 0 on buyAmount (numerator) is a Market Sell Order.
    let buy_amount = if denominator > 0 {
        // up-casting to handle overflow
        let top = U256::from(remaining) * U256::from(numerator);
        (top).ceiled_div(U256::from(denominator)).as_u128()
    } else {
        0
    };
    (buy_amount, remaining)
}

#[cfg(test)]
pub mod tests {
    use super::*;

    fn emptyish_auction_element() -> StableXAuctionElement {
        StableXAuctionElement {
            valid_from: U256::from(0),
            valid_until: U256::from(0),
            sell_token_balance: 0,
            order: Order {
                id: 0,
                account_id: Address::from_low_u64_be(0),
                buy_token: 0,
                sell_token: 0,
                buy_amount: 0,
                sell_amount: 0,
            },
        }
    }

    #[test]
    fn null_auction_element_from_bytes() {
        let res = StableXAuctionElement::from_bytes(&[0u8; 112]);

        assert_eq!(res, emptyish_auction_element());
    }

    #[test]
    fn computation_of_buy_sell_amounts() {
        let numerator = 19;
        let denominator = 14;
        let remaining = 5;
        let result = compute_buy_sell_amounts(numerator, denominator, remaining);
        assert_eq!(result, ((5 * 19 + 13) / 14, 5));
    }
    #[test]
    fn custom_auction_element_from_bytes() {
        let bytes: [u8; 112] = [
            0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, // user: 20 elements
            0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            0, 0, 3, // sellTokenBalance: 3, 32 elements
            1, 2, // buyToken: 256+2,
            1, 1, // sellToken: 256+1,
            0, 0, 0, 2, // validFrom: 2
            0, 0, 1, 5, // validUntil: 256+5
            0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 2, // priceNumerator: 258
            0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 3, // priceDenominator: 259
            0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 1, // remainingAmount: 2**8 + 1 = 257
        ];
        let res = StableXAuctionElement::from_bytes(&bytes);
        let auction_element = StableXAuctionElement {
            valid_from: U256::from(2),
            valid_until: U256::from(261),
            sell_token_balance: 3,
            order: Order {
                id: 0,
                account_id: Address::from_low_u64_be(1),
                buy_token: 258,
                sell_token: 257,
                buy_amount: (258 * 257 + 258) / 259,
                sell_amount: 257,
            },
        };
        assert_eq!(res, auction_element);
    }

    #[test]
    fn test_index_auction_element() {
        let bytes: [u8; 114] = [
            0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, // user: 20 elements
            0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            0, 0, 3, // sellTokenBalance: 3, 32 elements
            1, 2, // buyToken: 256+2,
            1, 1, // sellToken: 256+1,
            0, 0, 0, 2, // validFrom: 2
            0, 0, 1, 5, // validUntil: 256+5
            0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 2, // priceNumerator: 258
            0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 3, // priceDenominator: 259
            0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 1, // remainingAmount: 2**8 + 1 = 257
            0, 1, // order index
        ];
        let res = StableXAuctionElement::from_indexed_bytes(&bytes);
        let auction_element = StableXAuctionElement {
            valid_from: U256::from(2),
            valid_until: U256::from(261),
            sell_token_balance: 3,
            order: Order {
                id: 1,
                account_id: Address::from_low_u64_be(1),
                buy_token: 258,
                sell_token: 257,
                buy_amount: (258 * 257 + 258) / 259,
                sell_amount: 257,
            },
        };
        assert_eq!(res, auction_element);
    }

    #[test]
    #[should_panic]
    fn test_from_bytes_fails_on_hopefully_null() {
        StableXAuctionElement::from_bytes(&[1u8; 112]);
    }

    // Testing in_auction
    #[test]
    fn not_in_auction_left() {
        let mut element = emptyish_auction_element();
        element.valid_from = U256::from(2);
        element.valid_until = U256::from(5);
        assert_eq!(element.in_auction(U256::from(1)), false);
    }

    #[test]
    fn not_in_auction_right() {
        let mut element = emptyish_auction_element();
        element.valid_from = U256::from(2);
        element.valid_until = U256::from(5);
        assert_eq!(element.in_auction(U256::from(6)), false);
    }

    #[test]
    fn in_auction_interior() {
        let mut element = emptyish_auction_element();
        element.valid_from = U256::from(2);
        element.valid_until = U256::from(5);
        assert_eq!(element.in_auction(U256::from(3)), true);
    }

    #[test]
    fn in_auction_boundary() {
        let mut element = emptyish_auction_element();
        element.valid_from = U256::from(2);
        element.valid_until = U256::from(5);
        assert_eq!(
            element.in_auction(U256::from(5)),
            true,
            "failed on right boundary"
        );
        assert_eq!(
            element.in_auction(U256::from(2)),
            true,
            "failed on left boundary"
        );
    }

    // tests for compute_buy_sell_amounts
    #[test]
    fn compute_buy_sell_tiny_numbers() {
        let numerator = 1u128;
        let denominator = 3u128;
        let remaining = 2u128;
        // Note that contract does not allow remaining > denominator!
        let (buy, sell) = compute_buy_sell_amounts(numerator, denominator, remaining);
        // Sell at most 2 for at least 1 (i.e. as long as the price at least 1:3)
        assert_eq!((buy, sell), (1, remaining));
    }

    #[test]
    fn compute_buy_sell_recoverable_overflow() {
        let numerator = 2u128;
        let denominator = u128::max_value();
        let remaining = u128::max_value();

        // Note that contract does not allow remaining > denominator!
        let (buy, sell) = compute_buy_sell_amounts(numerator, denominator, remaining);
        // Sell at most 3 for at least 2 (i.e. as long as the price at least 1:2)
        assert_eq!((buy, sell), (2, remaining));
    }

    #[test]
    fn generic_compute_buy_sell() {
        let (numerator, denominator) = (1_000u128, 1_500u128);
        let remaining = 1_486u128;
        let (buy, sell) = compute_buy_sell_amounts(numerator, denominator, remaining);
        assert_eq!((buy, sell), (991, remaining));
    }

    #[test]
    fn generic_compute_buy_sell_2() {
        let (numerator, denominator) = (1_000u128, 1_500u128);
        let remaining = 1_485u128;
        let (buy, sell) = compute_buy_sell_amounts(numerator, denominator, remaining);
        assert_eq!((buy, sell), (990, remaining));
    }
}
