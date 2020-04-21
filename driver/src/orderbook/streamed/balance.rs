use super::*;
use anyhow::{anyhow, ensure, Result};
use serde::{Deserialize, Serialize};

/// Balance change from a potential solution that might still get replaced by a better solution in
/// the same batch.
#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize)]
struct Proceeds {
    batch_id: BatchId,
    // Two fields instead of one I256 because although unlikely it could be possible to overflow a
    // I256 with multiple trades in one batch.
    increase: U256,
    decrease: U256,
}

enum TradeType {
    Sell,
    Buy,
}

impl Proceeds {
    fn get_field(&mut self, operation: TradeType) -> &mut U256 {
        match operation {
            TradeType::Sell => &mut self.decrease,
            TradeType::Buy => &mut self.increase,
        }
    }
}

/// The balance of a token for a user.
#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize)]
pub struct Balance {
    balance: U256,
    deposit: Flux,
    withdraw: Flux,
    proceeds: Proceeds,
}

impl Balance {
    pub fn deposit(&mut self, amount: U256, batch_id: BatchId) -> Result<()> {
        self.apply_existing_deposit_and_proceeds(batch_id)?;
        // Works like in the smart contract: If there is an existing deposit we override the
        // batch id and add to the amount.
        self.deposit.batch_id = batch_id;
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
        self.apply_existing_deposit_and_proceeds(batch_id)?;
        self.balance = self
            .balance
            .checked_sub(amount)
            .ok_or_else(|| anyhow!("withdraw more than balance"))?;
        Ok(())
    }

    pub fn get_balance(&self, batch_id: BatchId) -> Result<U256> {
        let mut balance = self.balance_with_deposit_and_proceeds(batch_id)?;
        if self.withdraw.batch_id < batch_id {
            // Saturating because withdraw requests can be for amounts larger than balance.
            balance = balance.saturating_sub(self.withdraw.amount);
        }
        Ok(balance)
    }

    pub fn sell(&mut self, amount: u128, batch_id: BatchId) -> Result<()> {
        self.sell_buy_internal(amount, batch_id, TradeType::Sell)
    }

    pub fn buy(&mut self, amount: u128, batch_id: BatchId) -> Result<()> {
        self.sell_buy_internal(amount, batch_id, TradeType::Buy)
    }

    pub fn revert_sell(&mut self, amount: u128, batch_id: BatchId) -> Result<()> {
        self.revert_sell_buy_internal(amount, batch_id, TradeType::Sell)
    }

    pub fn revert_buy(&mut self, amount: u128, batch_id: BatchId) -> Result<()> {
        self.revert_sell_buy_internal(amount, batch_id, TradeType::Buy)
    }

    fn sell_buy_internal(
        &mut self,
        amount: u128,
        batch_id: BatchId,
        operation: TradeType,
    ) -> Result<()> {
        ensure!(self.proceeds.batch_id <= batch_id, "trade for past batch");
        self.apply_existing_deposit_and_proceeds(batch_id)?;
        // Now old proceeds have been cleared. If there is an existing proceed for this batch then
        // setting the batch_id does nothing and we add to the field.
        self.proceeds.batch_id = batch_id;
        let field = self.proceeds.get_field(operation);
        *field = field
            .checked_add(amount.into())
            .ok_or_else(|| anyhow!("math overflow"))?;
        Ok(())
    }

    fn revert_sell_buy_internal(
        &mut self,
        amount: u128,
        batch_id: BatchId,
        operation: TradeType,
    ) -> Result<()> {
        ensure!(
            self.proceeds.batch_id == batch_id,
            "reverting non existent trade"
        );
        let field = self.proceeds.get_field(operation);
        *field = field
            .checked_sub(amount.into())
            .ok_or_else(|| anyhow!("math underflow"))?;
        Ok(())
    }

    fn balance_with_deposit_and_proceeds(&self, current_batch_id: BatchId) -> Result<U256> {
        let mut balance = self.balance;
        if self.deposit.batch_id < current_batch_id {
            balance = balance
                .checked_add(self.deposit.amount)
                .ok_or_else(|| anyhow!("math overflow"))?;
        }
        if self.proceeds.batch_id < current_batch_id {
            balance = balance
                .checked_add(self.proceeds.increase)
                .ok_or_else(|| anyhow!("math overflow"))?
                .checked_sub(self.proceeds.decrease)
                .ok_or_else(|| anyhow!("math underflow"))?;
        }
        Ok(balance)
    }

    fn apply_existing_deposit_and_proceeds(&mut self, current_batch_id: BatchId) -> Result<()> {
        self.balance = self.balance_with_deposit_and_proceeds(current_batch_id)?;
        if self.deposit.batch_id < current_batch_id {
            self.deposit.amount = U256::zero();
        }
        if self.proceeds.batch_id < current_batch_id {
            self.proceeds.increase = U256::zero();
            self.proceeds.decrease = U256::zero();
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
