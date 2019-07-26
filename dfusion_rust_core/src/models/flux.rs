use byteorder::{BigEndian, WriteBytesExt};
use graph::data::store::Entity;
use serde_derive::{Deserialize, Serialize};
use std::sync::Arc;
use web3::types::{H256, U256, Log};

use crate::models::{Serializable, RootHashable, merkleize};
use super::util::*;

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
            account_id: pop_u16_from_log_data(&mut bytes),
            token_id: pop_u8_from_log_data(&mut bytes),
            amount: pop_u128_from_log_data(&mut bytes),
            slot: pop_u256_from_log_data(&mut bytes),
            slot_index: pop_u16_from_log_data(&mut bytes),
        }
    }
}

impl From<Entity> for PendingFlux {
    fn from(entity: Entity) -> Self {
        PendingFlux {
            account_id: u16::from_entity(&entity, "accountId"),
            token_id: u8::from_entity(&entity, "tokenId"),
            amount: u128::from_entity(&entity, "amount"),
            slot: U256::from_entity(&entity, "slot"),
            slot_index: u16::from_entity(&entity, "slotIndex"),
        }
    }
}

impl Into<Entity> for PendingFlux {
    fn into(self) -> Entity {
        let mut entity = Entity::new();
        entity.set("accountId", i32::from(self.account_id));
        entity.set("tokenId", i32::from(self.token_id));
        entity.set("amount", to_value(&self.amount));
        entity.set("slot", to_value(&self.slot));
        entity.set("slotIndex", i32::from(self.slot_index));
        entity
    }
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
  use graph::bigdecimal::BigDecimal;
  use web3::types::{H256, Bytes};
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

  #[test]
  fn test_from_log() {
      let bytes: Vec<Vec<u8>> = vec![
        /* account_id_bytes */ vec![0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1],
        /* token_id_bytes */ vec![ 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1],
        /* amount_bytes */ vec![0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 13, 224, 182, 179, 167, 100, 0, 0],
        /* slot_bytes */ vec![0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
        /* slot_index_bytes */ vec![0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
      ];

      let log = Arc::new(Log {
            address: 1.into(),
            topics: vec![],
            data: Bytes(bytes.iter().flat_map(|i| i.iter()).cloned().collect()),
            block_hash: Some(2.into()),
            block_number: Some(1.into()),
            transaction_hash: Some(3.into()),
            transaction_index: Some(0.into()),
            log_index: Some(0.into()),
            transaction_log_index: Some(0.into()),
            log_type: None,
            removed: None,
        });

        let expected_flux = PendingFlux {
            account_id: 1,
            token_id: 1,
            amount: 1 * (10 as u128).pow(18),
            slot: U256::zero(),
            slot_index: 0,
        };

        assert_eq!(expected_flux, PendingFlux::from(log));
  }

  #[test]
  fn test_to_and_from_entity() {
      let flux = PendingFlux {
            account_id: 1,
            token_id: 1,
            amount: 1 * (10 as u128).pow(18),
            slot: U256::zero(),
            slot_index: 0,
        };
        
        let mut entity = Entity::new();
        entity.set("accountId", 1);
        entity.set("tokenId", 1);
        entity.set("amount", BigDecimal::from(1 * (10 as u64).pow(18)));
        entity.set("slot", BigDecimal::from(0));
        entity.set("slotIndex", 0);

        assert_eq!(entity, flux.clone().into());
        assert_eq!(flux, PendingFlux::from(entity));
  }
}