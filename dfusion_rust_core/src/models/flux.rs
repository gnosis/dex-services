use byteorder::{BigEndian, WriteBytesExt};
use graph::bigdecimal::BigDecimal;
use graph::data::store::{Entity};
use serde_derive::{Deserialize, Serialize};
use std::convert::TryInto;
use std::str::FromStr;
use std::sync::Arc;
use web3::types::{H256, U256, Log};

use crate::models::{Serializable, RootHashable, merkleize};

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug, Ord, PartialOrd, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PendingFlux {
  pub slot_index: u16,
  pub slot: U256,
  pub account_id: u16,
  pub token_id: u8,
  pub amount: u128,
}

impl Serializable for PendingFlux {
  fn bytes(&self) -> Vec<u8> {
    let mut wtr = vec![0; 13];
    wtr.write_u16::<BigEndian>(self.account_id).unwrap();
    wtr.write_u8(self.token_id).unwrap();
    wtr.write_u128::<BigEndian>(self.amount).unwrap();
    wtr
  }
}

impl From<mongodb::ordered::OrderedDocument> for PendingFlux {
    fn from(document: mongodb::ordered::OrderedDocument) -> Self {
        PendingFlux {
            slot_index: document.get_i32("slotIndex").unwrap() as u16,
            slot: U256::from(document.get_i32("slot").unwrap()),
            account_id: document.get_i32("accountId").unwrap() as u16,
            token_id: document.get_i32("tokenId").unwrap() as u8,
            amount: document.get_str("amount").unwrap().parse().unwrap(),
        }
    }
}

impl From<Arc<Log>> for PendingFlux {
    fn from(log: Arc<Log>) -> Self {
        let mut bytes: Vec<u8> = log.data.0.clone();
        PendingFlux {
            account_id: read_u16(&mut bytes),
            token_id: read_u8(&mut bytes),
            amount: read_u128(&mut bytes),
            slot: read_u256(&mut bytes),
            slot_index: read_u16(&mut bytes),
        }
    }
}

impl Into<Entity> for PendingFlux {
    fn into(self) -> Entity {
        let mut entity = Entity::new();
        entity.set("accountId", self.account_id as i32);
        entity.set("tokenId", self.token_id as i32);
        entity.set("amount", BigDecimal::from_str(&self.amount.to_string()).unwrap());
        entity.set("slot", BigDecimal::from_str(&self.slot.to_string()).unwrap());
        entity.set("slotIndex", self.slot_index as i32);
        entity
    }
}

fn read_u8(bytes: &mut Vec<u8>) -> u8 {
    read_u256(bytes).as_u32().try_into().unwrap()
}

fn read_u16(bytes: &mut Vec<u8>) -> u16 {
    read_u256(bytes).as_u32().try_into().unwrap()
}

fn read_u128(bytes: &mut Vec<u8>) -> u128 {
    read_u256(bytes).to_string().parse().unwrap()
}

fn read_u256(bytes: &mut Vec<u8>) -> U256 {
    U256::from_big_endian(
        bytes.drain(0..32).collect::<Vec<u8>>().as_slice()
    )
}

impl RootHashable for Vec<PendingFlux> {
    fn root_hash(&self, valid_items: &[bool]) -> H256 {
        assert_eq!(self.len(), valid_items.len());
        let mut withdraw_bytes = vec![vec![0; 32]; 128];
        for (index, _) in valid_items.iter().enumerate().filter(|(_, valid)| **valid) {
            withdraw_bytes[index] = self[index].bytes();
        }
        merkleize(withdraw_bytes)
    }
}

pub mod tests {
    use super::*;
    pub fn create_flux_for_test(slot: u32, slot_index: u16) -> PendingFlux {
        PendingFlux {
            slot_index,
            slot: U256::from(slot),
            account_id: 1,
            token_id: 1,
            amount: 10,
        }
    }
}

#[cfg(test)]
pub mod unit_test {
  use super::*;
  use web3::types::{H256};
  use std::str::FromStr;

  #[test]
  fn test_pending_flux_root_hash() {
    let deposit = PendingFlux {
      slot_index: 0,
      slot: U256::zero(),
      account_id: 3,
      token_id: 3,
      amount: 18,
    };
    // one valid withdraw
    assert_eq!(
        vec![deposit.clone()].root_hash(&vec![true]),
        H256::from_str(
            "4a77ba0bc619056248f2f2793075eb6f49cf35dacb5cccfe1e71392046a06b79"
        ).unwrap()
    );
    // no valid withdraws
    assert_eq!(
        vec![deposit].root_hash(&vec![false]),
        H256::from_str(
            "87eb0ddba57e35f6d286673802a4af5975e22506c7cf4c64bb6be5ee11527f2c"
        ).unwrap()
    );
  }
}