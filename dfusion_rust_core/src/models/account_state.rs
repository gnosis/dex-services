use byteorder::{BigEndian, WriteBytesExt};
use graph::data::store::Entity;
use serde_derive::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{hash_map::Entry, HashMap};
use std::sync::Arc;
use web3::types::{Log, H256, U256};

use crate::models::{Order, PendingFlux, RollingHashable, Solution};

use super::util::*;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AccountState {
    pub state_hash: H256,
    pub state_index: U256,
    balances: HashMap<U256, HashMap<u8, u128>>, // UserId => (TokenId => balance)
    pub num_tokens: u8,
}

impl AccountState {
    pub fn new(state_hash: H256, state_index: U256, balances: Vec<u128>, num_tokens: u8) -> Self {
        AccountState {
            state_hash,
            state_index,
            balances: balances
                .chunks(num_tokens as usize)
                .enumerate()
                .map(|(account, token_balances)| {
                    (
                        U256::from(account),
                        token_balances
                            .iter()
                            .enumerate()
                            .map(|(token, balance)| (token as u8, *balance))
                            .collect(),
                    )
                })
                .collect(),
            num_tokens,
        }
    }
    pub fn get_balance_vector(&self) -> Vec<u128> {
        let mut i = U256::zero();
        let mut result = vec![];
        while let Some(account) = self.balances.get(&i) {
            let mut j = 0;
            while let Some(token_balance) = account.get(&j) {
                result.push(*token_balance);
                j += 1;
            }
            assert_eq!(j, self.num_tokens);
            i += U256::one();
        }
        result
    }
    pub fn read_balance(&self, token_id: u8, account_id: u16) -> u128 {
        *self
            .balances
            .get(&U256::from(account_id))
            .and_then(|token_balance| token_balance.get(&token_id))
            .unwrap_or(&0)
    }
    pub fn increment_balance(&mut self, token_id: u8, account_id: u16, amount: u128) {
        debug!(
            "Incrementing account {} balance of token {} by {}",
            account_id, token_id, amount
        );
        self.modify_balance(account_id, token_id, |balance| *balance += amount);
    }
    pub fn decrement_balance(&mut self, token_id: u8, account_id: u16, amount: u128) {
        debug!(
            "Decrementing account {} balance of token {} by {}",
            account_id, token_id, amount
        );
        self.modify_balance(account_id, token_id, |balance| *balance -= amount);
    }
    pub fn accounts(&self) -> u16 {
        let balance_vector = self.get_balance_vector();
        assert_eq!(
            balance_vector.len() % self.num_tokens as usize,
            0,
            "Balance vector cannot be split into equal accounts"
        );
        (balance_vector.len() / self.num_tokens as usize) as u16
    }

    pub fn apply_deposits(&mut self, deposits: &[PendingFlux]) {
        for deposit in deposits {
            self.increment_balance(deposit.token_id, deposit.account_id, deposit.amount)
        }
        self.state_index = self.state_index.saturating_add(U256::one());
        self.state_hash = self.rolling_hash(self.state_index.low_u32());
    }

    pub fn apply_withdraws(&mut self, withdraws: &[PendingFlux]) -> Vec<bool> {
        let mut valid_withdraws = vec![];
        for withdraw in withdraws {
            if self.read_balance(withdraw.token_id, withdraw.account_id) >= withdraw.amount {
                self.decrement_balance(withdraw.token_id, withdraw.account_id, withdraw.amount);
                valid_withdraws.push(true);
            } else {
                valid_withdraws.push(false);
            }
        }
        self.state_index = self.state_index.saturating_add(U256::one());
        self.state_hash = self.rolling_hash(self.state_index.low_u32());
        valid_withdraws
    }

    pub fn apply_auction(&mut self, orders: &[Order], results: Solution) {
        let buy_amounts = results.executed_buy_amounts;
        let sell_amounts = results.executed_sell_amounts;

        for (i, order) in orders.iter().enumerate() {
            self.increment_balance(order.buy_token, order.account_id, buy_amounts[i]);
            self.decrement_balance(order.sell_token, order.account_id, sell_amounts[i]);
        }
        self.state_index = self.state_index.saturating_add(U256::one());
        self.state_hash = self.rolling_hash(self.state_index.low_u32());
    }

    fn modify_balance<F>(&mut self, account_id: u16, token_id: u8, func: F)
    where
        F: FnOnce(&mut u128),
    {
        match self.balances.entry(U256::from(account_id)) {
            Entry::Occupied(mut account) => match account.get_mut().entry(token_id) {
                Entry::Occupied(mut balance) => func(balance.get_mut()),
                Entry::Vacant(_) => panic!(
                    "No balance for token {} at account {}",
                    token_id, account_id
                ),
            },
            Entry::Vacant(_) => panic!("No balances for account {}", account_id),
        };
    }
}

impl RollingHashable for AccountState {
    //Todo: Exchange sha with pederson hash
    fn rolling_hash(&self, nonce: u32) -> H256 {
        let mut hash = vec![0u8; 28];
        hash.write_u32::<BigEndian>(nonce).unwrap();
        for i in &self.get_balance_vector() {
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

impl From<Arc<Log>> for AccountState {
    fn from(log: Arc<Log>) -> Self {
        let mut bytes: Vec<u8> = log.data.0.clone();
        let state_hash = H256::pop_from_log_data(&mut bytes);
        let num_tokens = u8::pop_from_log_data(&mut bytes);
        let num_accounts = u16::pop_from_log_data(&mut bytes);
        let balances = vec![0; num_tokens as usize * num_accounts as usize];
        AccountState::new(state_hash, U256::zero(), balances, num_tokens)
    }
}

impl From<Entity> for AccountState {
    fn from(entity: Entity) -> Self {
        AccountState::new(
            H256::from_entity(&entity, "id"),
            U256::from_entity(&entity, "stateIndex"),
            Vec::from_entity(&entity, "balances"),
            u8::from_entity(&entity, "numTokens"),
        )
    }
}

impl Into<Entity> for AccountState {
    fn into(self) -> Entity {
        let mut entity = Entity::new();
        entity.set("id", self.state_hash.to_value());
        entity.set("stateIndex", self.state_index.to_value());
        entity.set("balances", self.get_balance_vector().to_value());
        entity.set("numTokens", self.num_tokens.to_value());
        entity
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use crate::models::order::BatchInformation;
    use crate::models::TOKENS;
    use web3::types::{Bytes, H256};

    #[test]
    fn test_state_rolling_hash() {
        // Empty state
        let mut balances = vec![0; 3000];
        let state_hash = "77b01abfbad57cb7a1344b12709603ea3b9ad803ef5ea09814ca212748f54733"
            .parse::<H256>()
            .unwrap();
        let state = AccountState::new(state_hash, U256::one(), balances.clone(), TOKENS);
        assert_eq!(state.rolling_hash(0), state_hash);

        // AccountState with single deposit
        balances[62] = 18;
        let state_hash = "a0cde336d10dbaf3df98ba662bacf25d95062db7b3e0083bd4bad4a6c7a1cd41"
            .parse::<H256>()
            .unwrap();
        let state = AccountState::new(state_hash, U256::one(), balances.clone(), TOKENS);
        assert_eq!(state.rolling_hash(0), state_hash);
    }

    #[test]
    fn test_from_log() {
        let bytes: Vec<Vec<u8>> = vec![
            /* state_hash */
            vec![
                0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                0, 0, 0, 0,
            ],
            /* num_tokens */
            vec![
                0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                0, 0, 0, 30,
            ],
            /* num_accounts */
            vec![
                0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                0, 0, 0, 100,
            ],
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

        let expected_state = AccountState::new(H256::zero(), U256::zero(), vec![0; 3000], TOKENS);
        assert_eq!(expected_state, AccountState::from(log));
    }

    #[test]
    fn test_to_and_from_entity() {
        let balances = vec![0, 18, 1];

        let state = AccountState::new(H256::zero(), U256::one(), balances.clone(), 3);

        let mut entity = Entity::new();
        entity.set("id", H256::zero().to_value());
        entity.set("stateIndex", U256::one().to_value());
        entity.set("balances", balances.to_value());
        entity.set("numTokens", 3u8.to_value());

        assert_eq!(entity, state.clone().into());
        assert_eq!(state, AccountState::from(entity));
    }

    #[test]
    fn test_apply_auction() {
        let balances = vec![0, 1, 1, 0];

        let mut state = AccountState::new(H256::zero(), U256::one(), balances.clone(), 2);

        let order_1 = Order {
            batch_information: Some(BatchInformation {
                slot: U256::zero(),
                slot_index: 0,
            }),
            account_id: 0,
            buy_token: 0,
            sell_token: 1,
            buy_amount: 1,
            sell_amount: 1,
        };

        let order_2 = Order {
            batch_information: Some(BatchInformation {
                slot: U256::zero(),
                slot_index: 0,
            }),
            account_id: 1,
            buy_token: 1,
            sell_token: 0,
            buy_amount: 1,
            sell_amount: 1,
        };

        let results = Solution {
            surplus: None,
            prices: vec![1, 1],
            executed_buy_amounts: vec![1, 1],
            executed_sell_amounts: vec![1, 1],
        };

        state.apply_auction(&[order_1, order_2], results);

        assert_eq!(
            H256::from("0x95d617036a54ec4e5081d096964aa6dc24dc33100c2f709b7df9917b3271de8d"),
            state.state_hash,
            "Incorrect state hash!"
        );
        assert_eq!(U256::from(2), state.state_index, "Incorrect state index!");
        let mut account_0 = HashMap::new();
        account_0.insert(0, 1);
        account_0.insert(1, 0);

        let mut account_1 = HashMap::new();
        account_1.insert(0, 0);
        account_1.insert(1, 1);

        let mut balances = HashMap::new();
        balances.insert(U256::zero(), account_0);
        balances.insert(U256::one(), account_1);
        assert_eq!(state.balances, balances, "Incorrect balances!");
    }

    #[test]
    #[should_panic]
    fn test_get_balance_vector_panics_on_spares_representation() {
        let mut account_0 = HashMap::new();
        account_0.insert(0, 1);

        let mut account_1 = HashMap::new();
        account_1.insert(1, 1);

        let mut balances = HashMap::new();
        balances.insert(U256::zero(), account_0);
        balances.insert(U256::one(), account_1);

        let account_state = AccountState {
            state_hash: H256::zero(),
            state_index: U256::zero(),
            balances,
            num_tokens: 2,
        };
        account_state.get_balance_vector();
    }
}
