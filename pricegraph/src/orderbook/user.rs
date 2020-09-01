//! Module implementing user and user token balance management.

use super::map::{self, Map};
use crate::encoding::{Element, TokenId, UserId};
use crate::num;

/// A type definiton for a mapping between user IDs to user data.
pub type UserMap = Map<UserId, User>;

/// User data containing balances and number of orders.
#[derive(Clone, Debug, Default)]
pub struct User {
    /// User balances per token.
    balances: Map<TokenId, f64>,
}

impl User {
    /// Adds an encoded orderbook element to the user data, including the
    /// reported balance and incrementing the number of orders.
    pub fn set_balance(&mut self, element: &Element) {
        self.balances
            .entry(element.pair.sell)
            .or_insert_with(|| num::u256_to_f64(element.balance));
    }

    /// Return's the user's balance for the specified token.
    pub fn balance_of(&self, token: TokenId) -> f64 {
        self.balances.get(&token).copied().unwrap_or(0.0)
    }

    /// Deducts an amount from the balance for the given token. Returns the new
    /// balance or `None` if the user no longer has any balance.
    pub fn deduct_from_balance(&mut self, token: TokenId, amount: f64) -> f64 {
        if let map::Entry::Occupied(mut entry) = self.balances.entry(token) {
            let balance = entry.get_mut();
            *balance -= amount;

            debug_assert!(
                *balance >= -num::max_rounding_error(amount),
                "user balance underflow for token {}",
                token,
            );

            if *balance > 0.0 {
                *balance
            } else {
                entry.remove_entry();
                0.0
            }
        } else {
            debug_assert!(
                false,
                "deducted amount from user with empty balace for token {}",
                token,
            );
            0.0
        }
    }

    /// Clears the user balance for a specific token.
    pub fn clear_balance(&mut self, token: TokenId) {
        self.balances.remove(&token);
    }
}
