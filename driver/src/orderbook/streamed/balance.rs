use super::*;
use anyhow::{anyhow, ensure, Result};
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
        batch_id: BatchId,
        current_batch_id: BatchId,
    ) -> Result<()> {
        ensure!(
            batch_id == current_batch_id,
            "deposit batch id does not match current batch id"
        );
        self.apply_existing_deposit(current_batch_id)?;
        // Works like in the smart contract: If there is an existing deposit we override the
        // batch id and add to the amount.
        self.deposit.batch_id = current_batch_id;
        self.deposit.amount = self
            .deposit
            .amount
            .checked_add(amount)
            .ok_or_else(|| anyhow!("overflow"))?;
        Ok(())
    }

    pub fn withdraw_request(
        &mut self,
        amount: U256,
        batch_id: BatchId,
        current_batch_id: BatchId,
    ) -> Result<()> {
        ensure!(batch_id >= current_batch_id, "withdraw request in the past");
        // It is not possible to get a new withdraw request when there already is an existing valid
        // withdraw request because the smart contract should have emitted a withdraw event for the
        // previous one first.
        ensure!(
            self.withdraw.batch_id >= current_batch_id || self.withdraw.amount == U256::zero(),
            "new withdraw request before clearing of previous withdraw request"
        );
        self.withdraw.batch_id = batch_id;
        self.withdraw.amount = amount;
        Ok(())
    }

    pub fn withdraw(&mut self, amount: U256, batch_id: BatchId) -> Result<()> {
        ensure!(
            self.withdraw.batch_id < batch_id,
            anyhow!("withdraw earlier than requested {}", self.withdraw.batch_id)
        );
        ensure!(
            self.withdraw.amount >= amount,
            anyhow!("withdraw more than requested {}", self.withdraw.amount)
        );
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
        let mut balance = self.balance_with_deposit(batch_id)?;
        if self.withdraw.batch_id < batch_id {
            // Saturating because withdraw requests can be for amounts larger than balance.
            balance = balance.saturating_sub(self.withdraw.amount);
        }
        Ok(balance)
    }

    fn balance_with_deposit(&self, current_batch_id: BatchId) -> Result<U256> {
        let mut balance = self.balance;
        if self.deposit.batch_id < current_batch_id {
            balance = balance
                .checked_add(self.deposit.amount)
                .ok_or_else(|| anyhow!("math overflow"))?;
        }
        Ok(balance)
    }

    fn apply_existing_deposit(&mut self, current_batch_id: BatchId) -> Result<()> {
        if self.deposit.batch_id < current_batch_id {
            self.balance = self.balance_with_deposit(current_batch_id)?;
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
