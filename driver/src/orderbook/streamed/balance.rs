use super::*;
use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};

/// The balance of a token for a user.
#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize)]
pub struct Balance {
    balance: U256,
    deposit: Flux,
    withdraw: Flux,
}

impl Balance {
    pub fn deposit(
        &mut self,
        amount: U256,
        deposit_batch_id: BatchId,
        current_batch_id: BatchId,
    ) -> Result<()> {
        self.apply_existing_deposit(current_batch_id)?;
        // Works like in the smart contract: If there is an existing deposit we override the
        // batch id and add to the amount. If there is no existing deposit then amount is already 0.
        let deposit_amount_including_this_deposit = self
            .deposit
            .amount
            .checked_add(amount)
            .ok_or_else(|| anyhow!("overflow"))?;
        self.deposit.batch_id = deposit_batch_id;
        self.deposit.amount = deposit_amount_including_this_deposit;
        Ok(())
    }

    pub fn withdraw_request(&mut self, amount: U256, batch_id: BatchId) {
        self.withdraw.batch_id = batch_id;
        self.withdraw.amount = amount;
    }

    pub fn withdraw(&mut self, amount: U256, batch_id: BatchId) -> Result<()> {
        if self.withdraw.batch_id >= batch_id {
            return Err(anyhow!(
                "withdraw earlier than requested {}",
                self.withdraw.batch_id
            ));
        }
        if self.withdraw.amount < amount {
            return Err(anyhow!(
                "withdraw more than requested {}",
                self.withdraw.amount
            ));
        }
        let balance_excluding_withdraw = self.get_balance_internal(batch_id, false)?;
        let new_balance = balance_excluding_withdraw
            .checked_sub(amount)
            .ok_or_else(|| anyhow!("withdraw more than balance {}", balance_excluding_withdraw))?;
        self.balance = new_balance;
        // If there is a valid deposit then new_balance already includes it so we need to clear it
        // now.
        self.clear_pending_deposit_if_possible(batch_id);
        // Works like in the smart contract: Any withdraw unconditionally removes the withdraw
        // request even if the amount is smaller than requested.
        self.withdraw.amount = 0.into();
        Ok(())
    }

    pub fn get_balance(&self, batch_id: BatchId) -> Result<U256> {
        self.get_balance_internal(batch_id, true)
    }

    fn get_balance_internal(&self, batch_id: BatchId, include_withdraw: bool) -> Result<U256> {
        let mut balance = self.balance;
        if self.deposit.batch_id < batch_id {
            balance = balance
                .checked_add(self.deposit.amount)
                .ok_or_else(|| anyhow!("math overflow"))?;
        }
        if include_withdraw && self.withdraw.batch_id < batch_id {
            // Saturating because withdraw requests can be for amounts larger than balance.
            balance = balance.saturating_sub(self.withdraw.amount);
        }
        Ok(balance)
    }

    fn apply_existing_deposit(&mut self, current_batch_id: BatchId) -> Result<()> {
        if self.deposit.batch_id < current_batch_id {
            let new_balance = self
                .balance
                .checked_add(self.deposit.amount)
                .ok_or_else(|| anyhow!("math overflow"))?;
            self.balance = new_balance;
            self.deposit.amount = U256::zero();
        }
        Ok(())
    }

    fn clear_pending_deposit_if_possible(&mut self, batch_id: BatchId) {
        if self.deposit.batch_id < batch_id {
            self.deposit.amount = 0.into();
        }
    }
}

/// A change in balance starting at some batch id.
#[derive(Copy, Clone, Debug, Default, Deserialize, Serialize)]
struct Flux {
    batch_id: BatchId,
    amount: U256,
}
