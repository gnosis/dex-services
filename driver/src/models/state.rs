use byteorder::{LittleEndian, WriteBytesExt};
use serde_derive::{Deserialize, Serialize};
use serde_json::json;
use sha2::{Digest, Sha256};
use web3::types::H256;
use std::collections::HashMap;

use crate::models::{TOKENS, RollingHashable, Order};
use crate::price_finding::{Solution};
use crate::price_finding::linear_optimization_price_finder::{account_id, token_id};

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct State {
    pub state_hash: String,
    pub state_index: i32,
    balances: Vec<u128>,
}

impl State {
    pub fn new(state_hash: String, state_index: i32, balances: Vec<u128>) -> Self {
        State { state_hash, state_index, balances }
    }
    fn balance_index(token_id: u8, account_id: u16) -> usize {
        TOKENS as usize * account_id as usize  + token_id as usize
    }
    pub fn read_balance(&self, token_id: u8, account_id: u16) -> u128 {
        self.balances[State::balance_index(token_id, account_id)]
    }
    pub fn increment_balance(&mut self, token_id: u8, account_id: u16, amount: u128) {
        self.balances[State::balance_index(token_id, account_id)] += amount;
    }
    pub fn decrement_balance(&mut self, token_id: u8, account_id: u16, amount: u128) {
        self.balances[State::balance_index(token_id, account_id)] -= amount;
    }
    pub fn serialize_balances(&self, num_tokens: u8) -> serde_json::Value {
        assert_eq!(
            self.balances.len() % num_tokens as usize, 0,
            "Balance vector cannot be split into equal accounts"
        );
        let mut accounts: HashMap<String, HashMap<String, String>> = HashMap::new();
        let mut current_account = 0;
        for account_balances in self.balances.chunks(num_tokens as usize) {
            accounts.insert(account_id(current_account), (0..num_tokens)
                .map(token_id)
                .zip(account_balances.iter().map(|b| b.to_string()))
                .collect());
            current_account += 1;
        }
        json!(accounts)
    }

    pub fn update_balances(&mut self, orders: &[Order], solution: &Solution) {
        for (i, order) in orders.iter().enumerate() {
            let buy_volume = solution.executed_buy_amounts[i];
            self.increment_balance(order.buy_token, order.account_id, buy_volume);

            let sell_volume = solution.executed_sell_amounts[i];
            self.decrement_balance(order.sell_token, order.account_id, sell_volume);
        }
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
        }
    }
}

#[cfg(test)]
pub mod tests {
  use super::*;
  use web3::types::{H256, U256};
  use std::str::FromStr;

    #[test]
    fn test_update_balances(){
        let mut state = State::new(
            "test".to_string(),
            0,
            vec![100; 70]
        );
        let solution = Solution {
            surplus: U256::from_dec_str("0").unwrap(),
            prices: vec![1, 2],
            executed_sell_amounts: vec![1, 1],
            executed_buy_amounts: vec![1, 1],
        };
        let order_1 = Order{
          slot_index: 1,
          account_id: 1,
          sell_token: 0,
          buy_token: 1,
          sell_amount: 4,
          buy_amount: 5,
        };
        let order_2 = Order{
          slot_index: 1,
          account_id: 0,
          sell_token: 1,
          buy_token: 0,
          sell_amount: 5,
          buy_amount: 4,
        };
        let orders = vec![order_1, order_2];

        state.update_balances(&orders, &solution);
        assert_eq!(state.read_balance(0, 0), 101);
        assert_eq!(state.read_balance(1, 0), 99);
        assert_eq!(state.read_balance(0, 1), 99);
        assert_eq!(state.read_balance(1, 1), 101);
    }

    #[test]
    fn test_serialize_balances() {
        let state = State::new(
            "test".to_string(),
            0,
            vec![100, 200, 300, 400, 500, 600]
        );
        let result = state.serialize_balances(3);
        let expected = json!({
            "account0": {
                "token0": "100",
                "token1": "200",
                "token2": "300",
            },
            "account1": {
                "token0": "400",
                "token1": "500",
                "token2": "600",
            }
        });
        assert_eq!(result, expected)
    }

    #[test]
    #[should_panic]
    fn test_serialize_balances_with_bad_balance_length() {
        let state = State::new(
            "test".to_string(),
            0,
            vec![100, 200]
        );
        state.serialize_balances( 3);
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
}