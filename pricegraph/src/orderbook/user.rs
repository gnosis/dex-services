//! Module implementing user and user token balance management.

use crate::encoding::{Element, TokenId, UserId};
use primitive_types::U256;
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
            .or_insert_with(|| u256_to_f64(element.balance));
        self.num_orders += 1;

        order_id
    }
}

/// Convert an unsigned 256-bit integer into a `f64`.
fn u256_to_f64(u: U256) -> f64 {
    let (u, factor) = match u {
        U256([_, _, 0, 0]) => (u, 1.0),
        U256([_, _, _, 0]) => (u >> 64, 2.0f64.powi(64)),
        U256([_, _, _, _]) => (u >> 128, 2.0f64.powi(128)),
    };
    (u.low_u128() as f64) * factor
}
