use byteorder::{BigEndian, WriteBytesExt};
use graph::data::store::{Entity, Value};
use serde_derive::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::sync::Arc;
use web3::types::{H256, U256, Log};

use crate::models::{TOKENS, RollingHashable};

use super::util::{pop_u8_from_log_data, pop_u16_from_log_data, pop_h256_from_log_data, to_value};

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
            state_hash: document.get_str("state_hash")
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

impl From<Arc<Log>> for State {
    fn from(log: Arc<Log>) -> Self {
        let mut bytes: Vec<u8> = log.data.0.clone();
        let state_hash = pop_h256_from_log_data(&mut bytes);
        let num_tokens = pop_u8_from_log_data(&mut bytes);
        let num_accounts = pop_u16_from_log_data(&mut bytes);
        State {
            state_hash,
            state_index: U256::zero(),
            num_tokens,
            balances: vec![0; num_tokens as usize * num_accounts as usize]
        }
    }
}

impl Into<Entity> for State {
    fn into(self) -> Entity {
        let mut entity = Entity::new();
        entity.set("id", format!("{:#x}", &self.state_hash));
        entity.set("stateIndex", to_value(&self.state_index));
        entity.set("balances", self.balances.iter().map(to_value).collect::<Vec<Value>>());
        entity.set("numTokens", i32::from(self.num_tokens));
        entity
    }
}

#[cfg(test)]
pub mod tests {
  use super::*;
  use web3::types::{H256, Bytes};

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

    #[test]
  fn test_from_log() {
      let bytes: Vec<Vec<u8>> = vec![
        /* state_hash */ vec![0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
        /* num_tokens */ vec![0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 30],
        /* num_accounts */ vec![0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 100],
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

        let expected_state = State {
            state_hash: H256::zero(),
            state_index:  U256::zero(),
            balances: vec![0; 3000],
            num_tokens: 30,
        };

        assert_eq!(expected_state, State::from(log));
  }

  #[test]
  fn test_to_entity() {
        let balances = vec![0, 18, 1];

        let state = State {
            state_hash: H256::zero(),
            state_index:  U256::one(),
            balances: balances.clone(),
            num_tokens: TOKENS,
        };
        
        let mut entity = Entity::new();
        entity.set("id", "0x0000000000000000000000000000000000000000000000000000000000000000");
        entity.set("stateIndex", to_value(&1));
        entity.set("balances", balances.iter().map(to_value).collect::<Vec<Value>>());
        entity.set("numTokens", i32::from(TOKENS));

        assert_eq!(entity, state.into());
  }
}