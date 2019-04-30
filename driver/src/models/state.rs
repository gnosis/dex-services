use byteorder::{LittleEndian, WriteBytesExt};
use serde_derive::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use web3::types::H256;

use crate::models::{TOKENS, RollingHashable};

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct State {
    pub state_hash: String,
    pub state_index: i32,
    balances: Vec<u128>,
    pub num_tokens: u8,
}

impl State {
    pub fn new(state_hash: String, state_index: i32, balances: Vec<u128>, num_tokens: u8) -> Self {
        State { state_hash, state_index, balances, num_tokens }
    }
    fn balance_index(&self, token_id: u8, account_id: u16) -> usize {
        self.num_tokens as usize * account_id as usize  + token_id as usize
    }
    pub fn read_balance(&self, token_id: u8, account_id: u16) -> u128 {
        self.balances[self.balance_index(token_id, account_id)]
    }
    pub fn increment_balance(&mut self, token_id: u8, account_id: u16, amount: u128) {
        let index = self.balance_index(token_id, account_id);
        self.balances[index] += amount;
    }
    pub fn decrement_balance(&mut self, token_id: u8, account_id: u16, amount: u128) {
        let index = self.balance_index(token_id, account_id);
        self.balances[index] -= amount;
    }
    pub fn accounts(&self) -> u16 {
        assert_eq!(
            self.balances.len() % self.num_tokens as usize, 0,
            "Balance vector cannot be split into equal accounts"
        );
        (self.balances.len() / self.num_tokens as usize) as u16
    }
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
            num_tokens: TOKENS,
        }
    }
}

#[cfg(test)]
pub mod tests {
  use super::*;
  use web3::types::{H256};
  use std::str::FromStr;

  #[test]
  fn test_state_rolling_hash() {
    // Empty state
    let mut balances = vec![0; 3000];
    let state = State {
        state_hash: "77b01abfbad57cb7a1344b12709603ea3b9ad803ef5ea09814ca212748f54733".to_string(),
        state_index:  1,
        balances: balances.clone(),
        num_tokens: TOKENS,
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
        num_tokens: TOKENS,
    };
    assert_eq!(
      state.rolling_hash(),
      H256::from_str(&state.state_hash).unwrap()
    );
  }
}