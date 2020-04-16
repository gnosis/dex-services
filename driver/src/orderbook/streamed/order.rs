use super::*;
use serde::{Deserialize, Serialize};

#[derive(Copy, Clone, Debug, Deserialize, Serialize)]
pub struct Order {
    pub buy_token: TokenId,
    pub sell_token: TokenId,
    pub valid_from: BatchId,
    pub valid_until: BatchId,
    pub price_numerator: u128,
    pub price_denominator: u128,
    pub used_amount: u128,
}

impl Order {
    pub fn has_limited_amount(&self) -> bool {
        self.price_numerator != std::u128::MAX && self.price_denominator != std::u128::MAX
    }
}
