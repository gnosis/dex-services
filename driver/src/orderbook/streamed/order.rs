use super::*;
use crate::contracts::stablex_auction_element;
use crate::models::Order as ModelOrder;
use anyhow::Result;
use serde::{Deserialize, Serialize};

/// Change from a potential solution that might still get replaced by a better solution in the same
/// batch.
#[derive(Copy, Clone, Debug, Deserialize, Serialize)]
struct PendingUsedAmount {
    batch_id: BatchId,
    amount: u128,
}

#[derive(Copy, Clone, Debug, Deserialize, Serialize)]
pub struct Order {
    pub buy_token: TokenId,
    pub sell_token: TokenId,
    pub valid_until: BatchId,
    pub valid_from: BatchId,
    price_numerator: u128,
    price_denominator: u128,
    // Invariant: used_amount + pending_used_amount.amount <= price_denominator
    used_amount: u128,
    pending_used_amount: PendingUsedAmount,
}

impl Order {
    pub fn new(
        buy_token: TokenId,
        sell_token: TokenId,
        valid_from: BatchId,
        valid_until: BatchId,
        price_numerator: u128,
        price_denominator: u128,
    ) -> Self {
        Self {
            buy_token,
            sell_token,
            valid_from,
            valid_until,
            price_numerator,
            price_denominator,
            used_amount: 0,
            pending_used_amount: PendingUsedAmount {
                batch_id: 0,
                amount: 0,
            },
        }
    }

    pub fn has_limited_amount(&self) -> bool {
        self.price_numerator != std::u128::MAX && self.price_denominator != std::u128::MAX
    }

    pub fn is_valid_in_batch(&self, batch_id: BatchId) -> bool {
        self.valid_from <= batch_id && batch_id <= self.valid_until
    }

    pub fn get_used_amount(&self, batch_id: BatchId) -> u128 {
        if self.pending_used_amount.batch_id < batch_id {
            // Cannot fail because of the invariant.
            self.used_amount
                .checked_add(self.pending_used_amount.amount)
                .unwrap()
        } else {
            self.used_amount
        }
    }

    pub fn get_remaining_amount(&self, batch_id: BatchId) -> u128 {
        // Cannot fail because of the invariant.
        self.price_denominator
            .checked_sub(self.get_used_amount(batch_id))
            .unwrap()
    }

    pub fn trade(&mut self, _used_amount: u128, _batch_id: BatchId) -> Result<()> {
        unimplemented!();
    }

    pub fn revert_trade(&mut self, _used_amount: u128, _batch_id: BatchId) -> Result<()> {
        unimplemented!();
    }

    pub fn as_model_order(
        &self,
        batch_id: BatchId,
        user_id: UserId,
        order_id: OrderId,
    ) -> ModelOrder {
        let (buy_amount, sell_amount) = stablex_auction_element::compute_buy_sell_amounts(
            self.price_numerator,
            self.price_denominator,
            self.get_remaining_amount(batch_id),
        );
        ModelOrder {
            id: order_id,
            account_id: user_id,
            buy_token: self.buy_token,
            sell_token: self.sell_token,
            buy_amount,
            sell_amount,
        }
    }
}
