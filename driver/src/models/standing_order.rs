use serde_derive::{Deserialize};
use sha2::{Digest, Sha256};
use web3::types::H256;
use crate::models::{ConcatenatingHashable, RollingHashable};
use crate::models;

#[derive(Debug, Clone, Deserialize, Eq, Ord, PartialEq, PartialOrd)]
#[serde(rename_all = "camelCase")]
pub struct StandingOrder {
    pub account_id: u16,
    pub batch_index: u32,
    orders: Vec<super::Order>,
}

impl StandingOrder {
    pub fn new(account_id: u16, batch_index: u32, orders: Vec<super::Order>) -> StandingOrder {
        StandingOrder { account_id, batch_index, orders }
    }
    pub fn get_orders(&self) -> &Vec<super::Order> {
        &self.orders
    }
    pub fn num_orders(&self) -> usize {
        self.orders.len()
    }
    pub fn empty(account_id: u16) -> StandingOrder {
        models::StandingOrder::new(account_id, 0, vec![])
    }
    pub fn initialize_array(&self) -> [StandingOrder; models::NUM_RESERVED_ACCOUNTS] {
        let mut array: [StandingOrder; models::NUM_RESERVED_ACCOUNTS] = unsafe { std::mem::uninitialized() };
        for i in array.iter_mut() {
            unsafe { ::std::ptr::write(i, self.clone()); }
        }
        array
    }
}

impl ConcatenatingHashable for [models::StandingOrder; models::NUM_RESERVED_ACCOUNTS] {
    fn concatenating_hash(&self, init_hash: H256) -> H256 {
       let mut hasher = Sha256::new();
        hasher.input(init_hash);
        self.iter().for_each(|k|  hasher.input(k.get_orders().rolling_hash(0)));
        let result = hasher.result();
        let b: Vec<u8> = result.to_vec();
        H256::from(b.as_slice())
    }
}

impl From<mongodb::ordered::OrderedDocument> for StandingOrder {
    fn from(document: mongodb::ordered::OrderedDocument) -> Self {
        let account_id = document.get_i32("_id").unwrap() as u16;
        let batch_index = document.get_i32("batchIndex").unwrap() as u32;
        StandingOrder {
            account_id,
            batch_index,
            orders: document
                .get_array("orders")
                .unwrap()
                .iter()
                .map(|raw_order| raw_order.as_document().unwrap())
                .map(|order_doc| super::Order {
                        account_id,
                        buy_token: order_doc.get_i32("buyToken").unwrap() as u8,
                        sell_token: order_doc.get_i32("sellToken").unwrap() as u8,
                        buy_amount: order_doc.get_str("buyAmount").unwrap().parse().unwrap(),
                        sell_amount: order_doc.get_str("sellAmount").unwrap().parse().unwrap(),
                    }
                ).collect()
        }
    }
}

#[cfg(test)]
pub mod tests {
  use super::*;
  use web3::types::{H256};
  use std::str::FromStr;

  #[test]
  fn test_concatenating_hash() {
    let standing_order = models::StandingOrder::new(
        1, 0, vec![create_order_for_test(), create_order_for_test()]
    );
    let mut standing_orders = models::StandingOrder::empty(0).initialize_array();
    standing_orders[1] = standing_order;
    assert_eq!(
    standing_orders.concatenating_hash(H256::from(0)),
    H256::from_str(
      "6bdda4f03645914c836a16ba8565f26dffb7bec640b31e1f23e0b3b22f0a64ae"
      ).unwrap()
    );
  }

  pub fn create_order_for_test() -> models::Order {
      models::Order {
          account_id: 1,
          sell_token: 2,
          buy_token: 3,
          sell_amount: 4,
          buy_amount: 5,
      }
    }
  }