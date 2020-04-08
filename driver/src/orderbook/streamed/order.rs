use super::*;
use error::Error;
use serde::{Deserialize, Serialize};

#[derive(Copy, Clone, Debug, Deserialize, Serialize)]
pub struct Order {
    pub buy_token: TokenId,
    pub sell_token: TokenId,
    pub valid_from: BatchId,
    pub valid_until: BatchId,
    pub price_numerator: u128,
    pub price_denominator: u128,
    used_amount: u128,
    pending_used_amount: (BatchId, u128),
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
            pending_used_amount: (0, 0),
        }
    }

    pub fn has_limited_amount(&self) -> bool {
        self.price_numerator != std::u128::MAX && self.price_denominator != std::u128::MAX
    }

    pub fn is_valid_in_batch(&self, batch_id: BatchId) -> bool {
        self.valid_from <= batch_id && batch_id <= self.valid_until
    }

    pub fn get_used_amount(&self, batch_id: BatchId) -> Result<u128, Error> {
        let used_amount = if self.pending_used_amount.0 < batch_id {
            self.used_amount
                .checked_add(self.pending_used_amount.1)
                .ok_or(Error::MathOverflow)?
        } else {
            self.used_amount
        };
        Ok(used_amount)
    }

    pub fn trade(&mut self, used_amount: u128, batch_id: BatchId) -> Result<(), Error> {
        if !self.has_limited_amount() {
            return Ok(());
        }
        self.used_amount = self.get_used_amount(batch_id)?;
        if self.pending_used_amount.0 == batch_id {
            self.pending_used_amount.1 = self
                .pending_used_amount
                .1
                .checked_add(used_amount)
                .ok_or(Error::MathOverflow)?;
        } else {
            self.pending_used_amount = (batch_id, used_amount);
        }
        Ok(())
    }

    pub fn revert_trade(&mut self, used_amount: u128, batch_id: BatchId) -> Result<(), Error> {
        if !self.has_limited_amount() {
            return Ok(());
        }
        if self.pending_used_amount.0 != batch_id {
            Err(Error::RevertingNonExistentTrade)
        } else {
            self.pending_used_amount.1 = self
                .pending_used_amount
                .1
                .checked_sub(used_amount)
                .ok_or(Error::MathOverflow)?;
            Ok(())
        }
    }
}
