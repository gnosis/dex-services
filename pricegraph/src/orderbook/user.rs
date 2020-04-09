//! Module implementing user and user token balance management.

use crate::encoding::{Element, TokenId, UserId};
use crate::num;
use std::collections::{hash_map, HashMap};

/// A type definiton for a mapping between user IDs to user data.
pub type UserMap = HashMap<UserId, User>;

/// User data containing balances and number of orders.
#[derive(Debug, Default, PartialEq)]
pub struct User {
    /// User balances per token.
    balances: HashMap<TokenId, f64>,
    /// The number of orders this user has.
    num_orders: usize,
}

impl User {
    /// Adds an encoded orderbook element to the user data, including the
    /// reported balance and incrementing the number of orders.
    pub fn include_order(&mut self, element: &Element) -> usize {
        let order_id = self.num_orders;

        self.balances
            .entry(element.pair.sell)
            .or_insert_with(|| num::u256_to_f64(element.balance));
        self.num_orders += 1;

        order_id
    }

    /// Return's the user's balance for the specified token.
    pub fn balance_of(&self, token: TokenId) -> f64 {
        self.balances.get(&token).copied().unwrap_or(0.0)
    }

    /// Deducts an amount from the balance for the given token. Returns the new
    /// balance or `None` if the user no longer has any balance.
    pub fn deduct_from_balance(&mut self, token: TokenId, amount: f64) -> Option<f64> {
        if let hash_map::Entry::Occupied(mut entry) = self.balances.entry(token) {
            let balance = entry.get_mut();
            *balance -= amount;

            if *balance > 0.0 {
                return Some(*balance);
            }
            entry.remove_entry();
        }
        None
    }
}
