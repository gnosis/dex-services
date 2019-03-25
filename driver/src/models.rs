use byteorder::{BigEndian, LittleEndian, WriteBytesExt};
use serde_derive::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use web3::types::H256;

pub const TOKENS: u16 = 30;
pub const DB_NAME: &str = "dfusion2";

pub trait RollingHashable {
    fn rolling_hash(&self) -> H256;
}

pub trait RootHashable {
    fn root_hash(&self, valid_items: &Vec<bool>) -> H256;
}

#[allow(non_snake_case)]
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct State {
  pub stateHash: String,
  pub stateIndex: i32,
  pub balances: Vec<u128>,
}

impl RollingHashable for State {
  //Todo: Exchange sha with pederson hash
  fn rolling_hash(&self) -> H256 {
    let mut hash = vec![0u8; 32];
    for i in &self.balances {
      let mut bs = [0u8; 32];
      bs.as_mut().write_u128::<LittleEndian>(*i).unwrap();

      let mut hasher = Sha256::new();
      hasher.input(hash);
      hasher.input(bs);
      let result = hasher.result();
      hash = result.to_vec();
    }
    H256::from(hash.as_slice())
  }
}

#[allow(non_snake_case)]
#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
pub struct PendingFlux {
  pub slotIndex: u32,
  pub slot: u32,
  pub accountId: u16,
  pub tokenId: u8,
  pub amount: u128,
}

impl PendingFlux {
  //calcalutes the iterative hash of deposits
  pub fn iter_hash(&self, prev_hash: &H256) -> H256 {
    let mut hasher = Sha256::new();
    hasher.input(prev_hash);
    hasher.input(self.bytes());
    let result = hasher.result();
    let b: Vec<u8> = result.to_vec();
    H256::from(b.as_slice())
  }

  pub fn bytes(&self) -> Vec<u8> {
    let mut wtr = vec![0; 13];
    wtr.write_u16::<BigEndian>(self.accountId).unwrap();
    wtr.write_u8(self.tokenId).unwrap();
    wtr.write_u128::<BigEndian>(self.amount).unwrap();
    wtr
  }
}

impl From<mongodb::ordered::OrderedDocument> for PendingFlux {
    fn from(document: mongodb::ordered::OrderedDocument) -> Self {
        let json = serde_json::to_string(&document).unwrap();
        serde_json::from_str(&json).unwrap()
    }
}

impl RollingHashable for Vec<PendingFlux> {
    fn rolling_hash(&self) -> H256 {
        self.iter().fold(H256::zero(), |acc, w| w.iter_hash(&acc))
    }
}

impl RootHashable for Vec<PendingFlux> {
    fn root_hash(&self, valid_items: &Vec<bool>) -> H256 {
        assert!(self.len() == valid_items.len());
        let mut withdraw_bytes = vec![vec![0; 32]; 128];
        for (index, _) in valid_items.iter().enumerate().filter(|(_, valid)| **valid) {
            withdraw_bytes[index] = self[index].bytes();
        }
        merkleize(withdraw_bytes)
    }
}

fn merkleize(leafs: Vec<Vec<u8>>) -> H256 {
    if leafs.len() == 1 {
        return H256::from(leafs[0].as_slice());
    }
    let next_layer = leafs.chunks(2).map(|pair| {
        let mut hasher = Sha256::new();
        hasher.input(&pair[0]);
        hasher.input(&pair[1]);
        hasher.result().to_vec()
    }).collect();
    merkleize(next_layer)
}

#[cfg(test)]
pub mod tests {
  use super::*;
  use web3::types::H256;
  use std::str::FromStr;

  #[test]
  fn test_pending_flux_rolling_hash() {
    let deposit = PendingFlux {
      slotIndex: 0,
      slot: 0,
      accountId: 0,
      tokenId: 0,
      amount: 0,
    };
    assert_eq!(
        vec![deposit].rolling_hash(),
        H256::from_str(
            "f5a5fd42d16a20302798ef6ed309979b43003d2320d9f0e8ea9831a92759fb4b"
        ).unwrap()
    );
    assert_eq!(
        vec![create_flux_for_test(0,0)].rolling_hash(), 
        H256::from_str(
            "dc8a5e14d3989bc687a8334e9096c515752e9726367ebab13b6f399865f71d3e"
        ).unwrap()
    );
  }

  #[test]
  fn test_pending_flux_root_hash() {
    let deposit = PendingFlux {
      slotIndex: 0,
      slot: 0,
      accountId: 3,
      tokenId: 3,
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
  fn test_state_rolling_hash() {
    // Empty state
    let mut balances = vec![0; 3000];
    let state = State {
        stateHash: "77b01abfbad57cb7a1344b12709603ea3b9ad803ef5ea09814ca212748f54733".to_string(),
        stateIndex:  1,
        balances: balances.clone(),
    };
    assert_eq!(
      state.rolling_hash(),
      H256::from_str(&state.stateHash).unwrap()
    );

    // State with single deposit
    balances[62] = 18;
    let state = State {
        stateHash: "73899d50b4ec5e351b4967e4c4e4a725e0fa3e8ab82d1bb6d3197f22e65f0c97".to_string(),
        stateIndex:  1,
        balances: balances,
    };
    assert_eq!(
      state.rolling_hash(),
      H256::from_str(&state.stateHash).unwrap()
    );
  }

  pub fn create_flux_for_test(slot: u32, slot_index: u32) -> PendingFlux {
      PendingFlux {
          slotIndex: slot_index,
          slot,
          accountId: 1,
          tokenId: 1,
          amount: 10,
      }
  }
}
