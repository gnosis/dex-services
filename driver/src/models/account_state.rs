use ethcontract::{Address as H160, H256, U256};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Serialize, Default, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AccountState {
    pub state_hash: H256,
    pub state_index: U256,
    balances: HashMap<H160, HashMap<u16, u128>>, // UserId => (TokenId => balance)
    pub num_tokens: u16,
}

impl AccountState {
    pub fn read_balance(&self, token_id: u16, account_id: H160) -> u128 {
        *self
            .balances
            .get(&account_id)
            .and_then(|token_balance| token_balance.get(&token_id))
            .unwrap_or(&0)
    }

    pub fn modify_balance<F>(&mut self, account_id: H160, token_id: u16, func: F)
    where
        F: FnOnce(&mut u128),
    {
        let account: &mut HashMap<u16, u128> =
            self.balances.entry(account_id).or_insert_with(HashMap::new);
        func(account.entry(token_id).or_insert(0));
    }
}

#[cfg(test)]
mod test_util {
    use super::*;
    use crate::models::Order;

    impl AccountState {
        pub fn new(
            state_hash: H256,
            state_index: U256,
            balances: Vec<u128>,
            num_tokens: u16,
        ) -> Self {
            assert_eq!(
                balances.len() % (num_tokens as usize),
                0,
                "Elements in balance vector needs to be a multiple of num_tokens"
            );
            AccountState {
                state_hash,
                state_index,
                balances: balances
                    .chunks(num_tokens as usize)
                    .enumerate()
                    .map(|(account, token_balances)| {
                        (
                            H160::from_low_u64_be(account as u64), // TODO - these are not accurate addresses.
                            token_balances
                                .iter()
                                .enumerate()
                                .map(|(token, balance)| (token as u16, *balance))
                                .collect(),
                        )
                    })
                    .collect(),
                num_tokens,
            }
        }

        pub fn with_balance_for(orders: &[Order]) -> Self {
            let mut state = AccountState {
                state_index: U256::zero(),
                state_hash: H256::zero(),
                balances: HashMap::new(),
                num_tokens: std::u16::MAX,
            };
            for order in orders {
                state.increment_balance(order.sell_token, order.account_id, order.sell_amount);
            }
            state
        }

        pub fn increment_balance(&mut self, token_id: u16, account_id: H160, amount: u128) {
            self.modify_balance(account_id, token_id, |balance| *balance += amount);
        }
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use ethcontract::H256;

    #[test]
    #[should_panic]
    fn test_cannot_create_with_bad_balance_length() {
        AccountState::new(H256::zero(), U256::zero(), vec![100, 200], 30);
    }
}
