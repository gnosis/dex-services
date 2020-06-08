use crate::models::Order;
use byteorder::{BigEndian, ByteOrder};
use ethcontract::{Address, U256};

pub const AUCTION_ELEMENT_WIDTH: usize = 112;
/// Indexed auction elements have the orderId appended at the end
pub const INDEXED_AUCTION_ELEMENT_WIDTH: usize = AUCTION_ELEMENT_WIDTH + 2;

#[derive(Debug, Default, PartialEq)]
pub struct StableXAuctionElement {
    pub sell_token_balance: U256,
    pub order: Order,
}

impl StableXAuctionElement {
    pub fn in_auction(&self, index: u32) -> bool {
        self.order.valid_from <= index && index <= self.order.valid_until
    }

    /// Deserialize an auction element that has been serialized by the smart
    /// contract's `encodeAuctionElement` function.
    /// Sets `id` to `0` because this information is not contained in the
    /// serialized information.
    pub fn from_bytes(bytes: &[u8; AUCTION_ELEMENT_WIDTH]) -> Self {
        let mut indexed_bytes = [0u8; INDEXED_AUCTION_ELEMENT_WIDTH];
        indexed_bytes[0..AUCTION_ELEMENT_WIDTH].copy_from_slice(bytes);
        Self::from_indexed_bytes(&indexed_bytes)
    }

    /// Deserialize an auction element that has been serialized by the smart
    /// contract's `getFilteredOrdersPaginated` function.
    pub fn from_indexed_bytes(bytes: &[u8; INDEXED_AUCTION_ELEMENT_WIDTH]) -> Self {
        let account_id = Address::from_slice(&bytes[0..20]);
        let sell_token_balance = U256::from_big_endian(&bytes[20..52]);
        let buy_token = u16::from_le_bytes([bytes[53], bytes[52]]);
        let sell_token = u16::from_le_bytes([bytes[55], bytes[54]]);
        let valid_from = u32::from_le_bytes([bytes[59], bytes[58], bytes[57], bytes[56]]);
        let valid_until = u32::from_le_bytes([bytes[63], bytes[62], bytes[61], bytes[60]]);
        let numerator = BigEndian::read_u128(&bytes[64..80]);
        let denominator = BigEndian::read_u128(&bytes[80..96]);
        let remaining = BigEndian::read_u128(&bytes[96..112]);
        let id = u16::from_le_bytes([bytes[113], bytes[112]]);
        StableXAuctionElement {
            sell_token_balance,
            order: Order {
                id,
                account_id,
                buy_token,
                sell_token,
                numerator,
                denominator,
                remaining_sell_amount: remaining,
                valid_from,
                valid_until,
            },
        }
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;

    #[test]
    fn null_auction_element_from_bytes() {
        let res = StableXAuctionElement::from_bytes(&[0u8; 112]);

        assert_eq!(res, StableXAuctionElement::default());
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
            sell_token_balance: U256::from(3),
            order: Order {
                id: 0,
                account_id: Address::from_low_u64_be(1),
                buy_token: 258,
                sell_token: 257,
                numerator: 258,
                denominator: 259,
                remaining_sell_amount: 257,
                valid_from: 2,
                valid_until: 261,
            },
        };
        assert_eq!(res, auction_element);
    }

    #[test]
    fn test_index_auction_element() {
        let bytes: [u8; 114] = [
            0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, // user: 20 elements
            128, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            0, 0, 0, 3, // sellTokenBalance: 3, 32 elements
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
            sell_token_balance: U256::from(2).pow(U256::from(255)) + 3,
            order: Order {
                id: 1,
                account_id: Address::from_low_u64_be(1),
                buy_token: 258,
                sell_token: 257,
                numerator: 258,
                denominator: 259,
                remaining_sell_amount: 257,
                valid_from: 2,
                valid_until: 261,
            },
        };
        assert_eq!(res, auction_element);
    }

    // Testing in_auction
    #[test]
    fn not_in_auction_left() {
        let mut element = StableXAuctionElement::default();
        element.order.valid_from = 2;
        element.order.valid_until = 5;
        assert_eq!(element.in_auction(1), false);
    }

    #[test]
    fn not_in_auction_right() {
        let mut element = StableXAuctionElement::default();
        element.order.valid_from = 2;
        element.order.valid_until = 5;
        assert_eq!(element.in_auction(6), false);
    }

    #[test]
    fn in_auction_interior() {
        let mut element = StableXAuctionElement::default();
        element.order.valid_from = 2;
        element.order.valid_until = 5;
        assert_eq!(element.in_auction(3), true);
    }

    #[test]
    fn in_auction_boundary() {
        let mut element = StableXAuctionElement::default();
        element.order.valid_from = 2;
        element.order.valid_until = 5;
        assert_eq!(element.in_auction(5), true, "failed on right boundary");
        assert_eq!(element.in_auction(2), true, "failed on left boundary");
    }
}
