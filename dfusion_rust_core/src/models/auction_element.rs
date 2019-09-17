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
        let account_id = H160::from(&bytes[92..112]);

        let hopefully_null = BigEndian::read_u128(&bytes[60..76]);

        assert_eq!(hopefully_null, 0);
        let sell_token_balance = BigEndian::read_u128(&bytes[76..92]);

        let buy_token = u16::from_le_bytes([bytes[58], bytes[59]]);
        let sell_token = u16::from_le_bytes([bytes[56], bytes[57]]);
        let valid_from = U256::from(u32::from_le_bytes([
            bytes[52], bytes[53], bytes[54], bytes[55],
        ]));
        let valid_until = U256::from(u32::from_le_bytes([
            bytes[48], bytes[49], bytes[50], bytes[51],
        ]));
        let is_sell_order = bytes[47] > 0;
        let numerator = BigEndian::read_u128(&bytes[31..47]);
        let denominator = BigEndian::read_u128(&bytes[15..31]);
        let remaining = BigEndian::read_u128(&bytes[0..15]);

        let (buy_amount, sell_amount) = if is_sell_order {
            (remaining, (numerator * remaining) / denominator)
        } else {
            ((denominator * remaining) / numerator, remaining)
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
    fn auction_element_from_bytes() {
        let null_auction_elt = AuctionElement::default();
        println!("{:?}", null_auction_elt);
        let mut order_count = HashMap::new();
        let res = AuctionElement::from_bytes(&mut order_count, &[0u8; 113]);
        println!("{:?}", res);

        assert_eq!(res, null_auction_elt);
    }
}
