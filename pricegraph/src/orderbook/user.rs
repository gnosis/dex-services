use super::map::{self, Map};
use crate::encoding::{Element, TokenId, UserId};
use primitive_types::U256;

/// A type definiton for a mapping between user IDs to user data.
pub type UserMap = Map<UserId, User>;

/// User data containing balances and number of orders.
#[derive(Clone, Debug, Default)]
pub struct User {
    /// User balances per token.
    balances: Map<TokenId, U256>,
}

impl User {
    /// Adds an encoded orderbook element to the user data, including the
    /// reported balance and incrementing the number of orders.
    pub fn set_balance(&mut self, element: &Element) {
        self.balances
            .entry(element.pair.sell)
            .or_insert_with(|| element.balance);
    }

    /// Return's the user's balance for the specified token.
    /// Panics if the user doesn't have a balance.
    pub fn balance_of(&self, token: TokenId) -> U256 {
        self.balances.get(&token).copied().unwrap_or_default()
    }

    /// Deducts an amount from the balance for the given token. Returns the new
    /// balance.
    pub fn deduct_from_balance(&mut self, token: TokenId, amount: U256) -> U256 {
        if let map::Entry::Occupied(mut entry) = self.balances.entry(token) {
            let balance = entry.get_mut();
            *balance = balance.saturating_sub(amount);
            if balance.is_zero() {
                entry.remove_entry();
                U256::zero()
            } else {
                *balance
            }
        } else {
            debug_assert!(
                false,
                "deducted amount from user with empty balace for token {}",
                token,
            );
            U256::zero()
        }
    }

    /// Clears the user balance for a specific token.
    pub fn clear_balance(&mut self, token: TokenId) {
        self.balances.remove(&token);
    }
}
