extern crate mock_it;

use byteorder::{BigEndian, ByteOrder};

use std::collections::HashMap;

use web3::types::{H160, U256};

use crate::models::{BatchInformation, Order};

#[derive(Debug, Default, PartialEq)]
pub struct AuctionElement {
    valid_from: U256,
    valid_until: U256,
    pub sell_token_balance: u128,
    pub order: Order,
}

impl AuctionElement {
    pub fn in_auction(&self, index: U256) -> bool {
        self.valid_from < index && index <= self.valid_until
    }

    pub fn from_bytes(order_count: &mut HashMap<H160, u16>, bytes: &[u8; 113]) -> Self {
        let account_id = H160::from(&bytes[93..]);

        // these go together (since sell_token_balance is emitted as u256 and treated as u128
        let sell_token_balance = BigEndian::read_u128(&bytes[77..93]);
        let hopefully_null = BigEndian::read_u128(&bytes[61..77]);
        assert_eq!(hopefully_null, 0, "User has too large balance to handle.");

        let buy_token = u16::from_le_bytes([bytes[59], bytes[60]]);
        let sell_token = u16::from_le_bytes([bytes[57], bytes[58]]);
        let valid_from = U256::from(u32::from_le_bytes([
            bytes[53], bytes[54], bytes[55], bytes[56],
        ]));
        let valid_until = U256::from(u32::from_le_bytes([
            bytes[49], bytes[50], bytes[51], bytes[52],
        ]));
        let is_sell_order = bytes[48] > 0;
        let numerator = BigEndian::read_u128(&bytes[32..48]);
        let denominator = BigEndian::read_u128(&bytes[16..32]);
        let remaining = BigEndian::read_u128(&bytes[0..16]);

        let mut other = 0;
        let (buy_amount, sell_amount) = if is_sell_order {
            if denominator > 0 {
                other = (numerator * remaining) / denominator;
            }
            (remaining, other)
        } else {
            if numerator > 0 {
                other = (denominator * remaining) / numerator;
            }
            (other, remaining)
        };

        // Increment order count for account.
        let counter = order_count.entry(account_id).or_insert(0);
        *counter += 1;

        AuctionElement {
            valid_from,
            valid_until,
            sell_token_balance,
            order: Order {
                batch_information: Some(BatchInformation {
                    slot_index: *counter,
                    slot: U256::from(0),
                }),
                account_id,
                buy_token,
                sell_token,
                buy_amount,
                sell_amount,
            },
        }
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;

    #[test]
    fn test_nearly_null_auction_element_from_bytes() {
        let mut nearly_null_auction_elt = AuctionElement::default();
        nearly_null_auction_elt.order.batch_information = Some(BatchInformation {
            slot_index: 1,
            slot: U256::from(0),
        });
        let mut order_count = HashMap::new();
        let res = AuctionElement::from_bytes(&mut order_count, &[0u8; 113]);

        assert_eq!(res, nearly_null_auction_elt);
    }

    #[test]
    #[should_panic]
    fn test_from_bytes_fails_on_hopefully_null() {
        let mut order_count = HashMap::new();
        AuctionElement::from_bytes(&mut order_count, &[1u8; 113]);
    }

    // Testing in_auction
    #[test]
    fn not_in_auction_left() {
        let mut element = AuctionElement::default();
        element.valid_from = U256::from(2);
        element.valid_until = U256::from(5);
        assert_eq!(element.in_auction(U256::from(2)), false);
    }

    #[test]
    fn not_in_auction_right() {
        let mut element = AuctionElement::default();
        element.valid_from = U256::from(2);
        element.valid_until = U256::from(5);
        assert_eq!(element.in_auction(U256::from(6)), false);
    }

    #[test]
    fn in_auction_interior() {
        let mut element = AuctionElement::default();
        element.valid_from = U256::from(2);
        element.valid_until = U256::from(5);
        assert_eq!(element.in_auction(U256::from(3)), true);
    }

    #[test]
    fn in_auction_boundary() {
        let mut element = AuctionElement::default();
        element.valid_from = U256::from(2);
        element.valid_until = U256::from(5);
        assert_eq!(element.in_auction(U256::from(5)), true);
    }

}
