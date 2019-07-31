use byteorder::{BigEndian, WriteBytesExt};
use graph::data::store::Entity;
use serde_derive::{Deserialize};
use std::sync::Arc;
use web3::types::{H256, U256, Log};

use crate::models::{Serializable, RollingHashable, iter_hash};

use super::util::*;

#[derive(Debug, Clone, Deserialize, Eq, Ord, PartialEq, PartialOrd)]
#[serde(rename_all = "camelCase")]
pub struct Order {
    pub slot: U256,
    pub slot_index: u16,
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

impl From<Arc<Log>> for Order {
    fn from(log: Arc<Log>) -> Self {
        let mut bytes: Vec<u8> = log.data.0.clone();
        Order {
            slot: U256::pop_from_log_data(&mut bytes),
            slot_index: u16::pop_from_log_data(&mut bytes),
            account_id: u16::pop_from_log_data(&mut bytes),
            sell_token: u8::pop_from_log_data(&mut bytes),
            buy_token: u8::pop_from_log_data(&mut bytes),
            sell_amount: u128::pop_from_log_data(&mut bytes),
            buy_amount: u128::pop_from_log_data(&mut bytes),
        }
    }
}

impl From<Entity> for Order {
    fn from(entity: Entity) -> Self {
        Order {
            slot: U256::from_entity(&entity, "auctionId"),
            slot_index: u16::from_entity(&entity, "slotIndex"),
            account_id: u16::from_entity(&entity, "accountId"),
            sell_token: u8::from_entity(&entity, "sellToken"),
            buy_token: u8::from_entity(&entity, "buyToken"),
            sell_amount: u128::from_entity(&entity, "sellAmount"),
            buy_amount: u128::from_entity(&entity, "buyAmount"),
        }
    }
}

impl From<mongodb::ordered::OrderedDocument> for Order {
    fn from(document: mongodb::ordered::OrderedDocument) -> Self {
        Order {
            slot: U256::from(document.get_i32("slot").unwrap()),
            slot_index: document.get_i32("slotIndex").unwrap() as u16,
            account_id: document.get_i32("accountId").unwrap() as u16,
            buy_token: document.get_i32("buyToken").unwrap() as u8,
            sell_token: document.get_i32("sellToken").unwrap() as u8,
            buy_amount: document.get_str("buyAmount").unwrap().parse().unwrap(),
            sell_amount: document.get_str("sellAmount").unwrap().parse().unwrap(),
        }
    }
}

impl Into<Entity> for Order {
    fn into(self) -> Entity {
        let mut entity = Entity::new();
        entity.set("slot", self.slot.to_value());
        entity.set("slotIndex", self.slot_index.to_value());
        entity.set("accountId", self.account_id.to_value());
        entity.set("buyToken", self.buy_token.to_value());
        entity.set("sellToken", self.sell_token.to_value());
        entity.set("sellAmount", self.sell_amount.to_value());
        entity.set("buyAmount", self.buy_amount.to_value());
        entity
    }
}

impl<T: Serializable> RollingHashable for Vec<T> {
    fn rolling_hash(&self, nonce: u32) -> H256 {
        self.iter().fold(H256::from(u64::from(nonce)), |acc, w| iter_hash(w, &acc))
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
          slot: U256::zero(),
          slot_index: 0,
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
      slot: U256::zero(),
      slot_index: 0,
    };

    assert_eq!(
    vec![order].rolling_hash(0),
    H256::from_str(
      "8c253b4588a6d87b02b5f7d1424020b7b5f8c0397e464e087d2830a126d3b699"
      ).unwrap()
    );
  }
}