//! Module implementing user and user token balance management.

use crate::encoding::{Element, TokenId, UserId};
use crate::num;
use std::collections::HashMap;

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

    /// Retrieves the user's balance for a token
    pub fn balance_of(&self, token: TokenId) -> f64 {
        self.balances.get(&token).copied().unwrap_or(0.0)
    }
}
