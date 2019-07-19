use byteorder::{BigEndian, WriteBytesExt};
use serde_derive::{Deserialize};
use web3::types::H256;

use crate::models::{Serializable, RollingHashable, iter_hash};

#[derive(Debug, Clone, Deserialize, Eq, Ord, PartialEq, PartialOrd)]
#[serde(rename_all = "camelCase")]
pub struct Order {
    pub account_id: u16,
    pub sell_token: u8,
    pub buy_token: u8,
    pub sell_amount: u128,
    pub buy_amount: u128,
}

impl Serializable for Order {
    fn bytes(&self) -> Vec<u8> {
        let mut wtr = vec![0; 4];
        wtr.extend(self.buy_amount.bytes());
        wtr.extend(self.sell_amount.bytes());
        wtr.write_u8(self.sell_token).unwrap();
        wtr.write_u8(self.buy_token).unwrap();
        wtr.write_u16::<BigEndian>(self.account_id).unwrap();
        wtr
    }
}

impl Serializable for u128 {
    fn bytes(&self) -> Vec<u8> {
        self.to_be_bytes()[4..].to_vec()
    }
}

impl From<mongodb::ordered::OrderedDocument> for Order {
    fn from(document: mongodb::ordered::OrderedDocument) -> Self {
        Order {
            account_id: document.get_i32("accountId").unwrap() as u16,
            buy_token: document.get_i32("buyToken").unwrap() as u8,
            sell_token: document.get_i32("sellToken").unwrap() as u8,
            buy_amount: document.get_str("buyAmount").unwrap().parse().unwrap(),
            sell_amount: document.get_str("sellAmount").unwrap().parse().unwrap(),
        }
    }
}

impl<T: Serializable> RollingHashable for Vec<T> {
    fn rolling_hash(&self, nonce: i32) -> H256 {
        self.iter().fold(H256::from(nonce as u64), |acc, w| iter_hash(w, &acc))
    }
}

pub mod tests {
    use super::*;
    pub fn create_order_for_test() -> Order {
      Order {
          account_id: 1,
          sell_token: 2,
          buy_token: 3,
          sell_amount: 4,
          buy_amount: 5,
      }
  }
}

#[cfg(test)]
pub mod unit_test {
  use super::*;
  use web3::types::{H256};
  use std::str::FromStr;

  #[test]
  fn test_order_rolling_hash() {
    let order = Order {
      account_id: 1,
      sell_token: 2,
      buy_token: 3,
      sell_amount: 4,
      buy_amount: 5,
    };

    assert_eq!(
    vec![order].rolling_hash(0),
    H256::from_str(
      "8c253b4588a6d87b02b5f7d1424020b7b5f8c0397e464e087d2830a126d3b699"
      ).unwrap()
    );
  }
}