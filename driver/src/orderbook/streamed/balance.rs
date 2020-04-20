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
    pub fn deposit(&mut self, amount: U256, current_batch_id: BatchId) -> Result<()> {
        self.apply_existing_deposit(current_batch_id)?;
        // Works like in the smart contract: If there is an existing deposit we override the
        // batch id and add to the amount. If there is no existing deposit then amount is already 0.
        let deposit_amount_including_this_deposit = self
            .deposit
            .amount
            .checked_add(amount)
            .ok_or_else(|| anyhow!("overflow"))?;
        self.deposit.batch_id = current_batch_id;
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
        // Works like in the smart contract: Any withdraw unconditionally removes the withdraw
        // request even if the amount is smaller than requested.
        self.withdraw.amount = 0.into();
        self.apply_existing_deposit(batch_id)?;
        self.balance = self
            .balance
            .checked_sub(amount)
            .ok_or_else(|| anyhow!("withdraw more than balance"))?;
        Ok(())
    }

    pub fn get_balance(&self, batch_id: BatchId) -> Result<U256> {
        let mut balance = self.balance;
        if self.deposit.batch_id < batch_id {
            balance = balance
                .checked_add(self.deposit.amount)
                .ok_or_else(|| anyhow!("math overflow"))?;
        }
        if self.withdraw.batch_id < batch_id {
            // Saturating because withdraw requests can be for amounts larger than balance.
            balance = balance.saturating_sub(self.withdraw.amount);
        }
        Ok(balance)
    }

    fn apply_existing_deposit(&mut self, current_batch_id: BatchId) -> Result<()> {
        if self.deposit.batch_id < current_batch_id {
            self.balance = self
                .balance
                .checked_add(self.deposit.amount)
                .ok_or_else(|| anyhow!("math overflow"))?;
            self.deposit.amount = U256::zero();
        }
        Ok(())
    }
}

/// A change in balance starting at some batch id.
#[derive(Copy, Clone, Debug, Default, Deserialize, Serialize)]
struct Flux {
    batch_id: BatchId,
    amount: U256,
}
