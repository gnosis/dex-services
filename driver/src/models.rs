use byteorder::{BigEndian, LittleEndian, WriteBytesExt};
use serde_derive::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use web3::types::H256;

pub const TOKENS: u8 = 30;
pub const DB_NAME: &str = "dfusion2";

pub trait RollingHashable {
    fn rolling_hash(&self) -> H256;
}

pub trait RootHashable {
    fn root_hash(&self, valid_items: &Vec<bool>) -> H256;
}

pub trait Serializable {
    fn bytes(&self) -> Vec<u8>;
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct State {
  pub state_hash: String,
  pub state_index: i32,
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

impl From<mongodb::ordered::OrderedDocument> for State {
    fn from(document: mongodb::ordered::OrderedDocument) -> Self {
        State {
            state_hash: document.get_str("stateHash").unwrap().to_owned(),
            state_index: document.get_i32("stateIndex").unwrap(),
            balances: document.get_array("balances")
                .unwrap()
                .iter()
                .map(|e| e.as_str().unwrap().parse().unwrap())
                .collect(),
        }
    }
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug, Ord, PartialOrd, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PendingFlux {
  pub slot_index: u32,
  pub slot: u32,
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
            slot_index: document.get_i32("slotIndex").unwrap() as u32,
            slot: document.get_i32("slot").unwrap() as u32,
            account_id: document.get_i32("accountId").unwrap() as u16,
            token_id: document.get_i32("tokenId").unwrap() as u8,
            amount: document.get_str("amount").unwrap().parse().unwrap(),
        }
    }
}

impl<T: Serializable> RollingHashable for Vec<T> {
    fn rolling_hash(&self) -> H256 {
        self.iter().fold(H256::zero(), |acc, w| iter_hash(w, &acc))
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

fn iter_hash<T: Serializable>(item: &T, prev_hash: &H256) -> H256 {
    let mut hasher = Sha256::new();
    hasher.input(prev_hash);
    hasher.input(item.bytes());
    let result = hasher.result();
    let b: Vec<u8> = result.to_vec();
    H256::from(b.as_slice())
  }

#[derive(Clone, Deserialize, Eq, Ord, PartialEq, PartialOrd)]
#[serde(rename_all = "camelCase")]
pub struct Order {
    pub slot_index: u32,
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
            slot_index: document.get_i32("slotIndex").unwrap() as u32,
            account_id: document.get_i32("accountId").unwrap() as u16,
            sell_token: document.get_i32("sellToken").unwrap() as u8,
            buy_token: document.get_i32("buyToken").unwrap() as u8,
            sell_amount: document.get_str("sellAmount").unwrap().parse().unwrap(),
            buy_amount: document.get_str("buyAmount").unwrap().parse().unwrap(),
        }
    }
}

#[cfg(test)]
pub mod tests {
  use super::*;
  use web3::types::H256;
  use std::str::FromStr;

  #[test]
  fn test_pending_flux_rolling_hash() {
    let deposit = PendingFlux {
      slot_index: 0,
      slot: 0,
      account_id: 0,
      token_id: 0,
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
      slot_index: 0,
      slot: 0,
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
  fn test_state_rolling_hash() {
    // Empty state
    let mut balances = vec![0; 3000];
    let state = State {
        state_hash: "77b01abfbad57cb7a1344b12709603ea3b9ad803ef5ea09814ca212748f54733".to_string(),
        state_index:  1,
        balances: balances.clone(),
    };
    assert_eq!(
      state.rolling_hash(),
      H256::from_str(&state.state_hash).unwrap()
    );

    // State with single deposit
    balances[62] = 18;
    let state = State {
        state_hash: "73899d50b4ec5e351b4967e4c4e4a725e0fa3e8ab82d1bb6d3197f22e65f0c97".to_string(),
        state_index:  1,
        balances,
    };
    assert_eq!(
      state.rolling_hash(),
      H256::from_str(&state.state_hash).unwrap()
    );
  }

  #[test]
  fn test_order_rolling_hash() {
    let order = Order {
      slot_index: 0,
      account_id: 1,
      sell_token: 2,
      buy_token: 3,
      sell_amount: 4,
      buy_amount: 5,
    };

    assert_eq!(
    vec![order].rolling_hash(),
    H256::from_str(
      "e1be57cc443a06d5b4e8c860eed65583e915cce10762f6f04a370326c187879b"
      ).unwrap()
    );
  }

  pub fn create_flux_for_test(slot: u32, slot_index: u32) -> PendingFlux {
      PendingFlux {
          slot_index,
          slot,
          account_id: 1,
          token_id: 1,
          amount: 10,
      }
  }
}
