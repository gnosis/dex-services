use byteorder::{BigEndian, WriteBytesExt};
use serde_derive::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use web3::types::{H256, U256};

use crate::models::{TOKENS, RollingHashable};

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct State {
    pub state_hash: H256,
    pub state_index: U256,
    balances: Vec<u128>,
    pub num_tokens: u8,
}

impl State {
    pub fn new(state_hash: H256, state_index: U256, balances: Vec<u128>, num_tokens: u8) -> Self {
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
  fn rolling_hash(&self, nonce: u32) -> H256 {
    let mut hash = vec![0u8; 28];
    hash.write_u32::<BigEndian>(nonce).unwrap();
    for i in &self.balances {
      let mut bs = vec![0u8; 16];
      bs.write_u128::<BigEndian>(*i).unwrap();

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
            state_hash: document.get_str("stateHash")
                .unwrap()
                .parse::<H256>()
                .unwrap(),
            state_index: U256::from(document.get_i32("stateIndex").unwrap()),
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

  #[test]
  fn test_state_rolling_hash() {
    // Empty state
    let mut balances = vec![0; 3000];
    let state_hash = "77b01abfbad57cb7a1344b12709603ea3b9ad803ef5ea09814ca212748f54733".parse::<H256>().unwrap();
    let state = State {
        state_hash: state_hash.clone(),
        state_index:  U256::one(),
        balances: balances.clone(),
        num_tokens: TOKENS,
    };
    assert_eq!(
      state.rolling_hash(0),
      state_hash
    );

    // State with single deposit
    balances[62] = 18;
    let state_hash = "a0cde336d10dbaf3df98ba662bacf25d95062db7b3e0083bd4bad4a6c7a1cd41".parse::<H256>().unwrap();
    let state = State {
        state_hash: state_hash.clone(),
        state_index:  U256::one(),
        balances,
        num_tokens: TOKENS,
    };
    assert_eq!(
      state.rolling_hash(0),
      state_hash
    );
  }
}