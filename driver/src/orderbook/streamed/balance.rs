use super::*;
use error::Error;
use serde::{Deserialize, Serialize};
use std::cmp::Ordering;

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize)]
pub struct Balance {
    balance: U256,
    deposit: Flux,
    withdraw: Flux,
    pending_solution: PendingSolutionBalance,
}

#[derive(Copy, Clone, Debug, Default, Deserialize, Serialize)]
struct Flux {
    batch_id: BatchId,
    amount: U256,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize)]
struct PendingSolutionBalance {
    batch_id: BatchId,
    increase: U256,
    decrease: U256,
}

impl PendingSolutionBalance {
    fn get_field(&mut self, operation: Operation) -> &mut U256 {
        match operation {
            Operation::Sell => &mut self.decrease,
            Operation::Buy => &mut self.increase,
        }
    }
}

enum Operation {
    Sell,
    Buy,
}

impl Balance {
    pub fn add_balance(&mut self, amount: U256) -> Result<(), Error> {
        self.balance = self
            .balance
            .checked_add(amount)
            .ok_or(Error::MathOverflow)?;
        Ok(())
    }

    pub fn deposit(
        &mut self,
        amount: U256,
        deposit_batch_id: BatchId,
        current_batch_id: BatchId,
    ) -> Result<(), Error> {
        if self.deposit.batch_id < current_batch_id {
            let new_balance = self
                .balance
                .checked_add(self.deposit.amount)
                .ok_or(Error::MathOverflow)?;
            self.balance = new_balance;
            self.deposit.amount = amount;
        } else {
            let new_deposit_amount = self
                .deposit
                .amount
                .checked_add(amount)
                .ok_or(Error::MathOverflow)?;
            self.deposit.batch_id = deposit_batch_id;
            self.deposit.amount = new_deposit_amount;
        }
        Ok(())
    }

    pub fn withdraw_request(&mut self, amount: U256, batch_id: BatchId) {
        self.withdraw.batch_id = batch_id;
        self.withdraw.amount = amount;
    }

    pub fn withdraw(&mut self, amount: U256, batch_id: BatchId) -> Result<(), Error> {
        if self.withdraw.batch_id >= batch_id {
            return Err(Error::WithdrawEarlierThanRequested(self.withdraw.batch_id));
        }
        if self.withdraw.amount < amount {
            return Err(Error::WithdrawMoreThanRequested(self.withdraw.amount));
        }
        let balance = self.get_balance_internal(batch_id, false)?;
        let new_balance = balance
            .checked_sub(amount)
            .ok_or(Error::WithdrawMoreThanBalance(balance))?;
        self.balance = new_balance;
        self.clear_pending_deposit_and_solution_if_possible(batch_id);
        self.withdraw.amount = 0.into();
        Ok(())
    }

    pub fn get_balance(&self, batch_id: BatchId) -> Result<U256, Error> {
        self.get_balance_internal(batch_id, true)
    }

    pub fn sell(&mut self, amount: u128, batch_id: BatchId) -> Result<(), Error> {
        self.sell_buy_internal(amount, batch_id, Operation::Sell)
    }

    pub fn buy(&mut self, amount: u128, batch_id: BatchId) -> Result<(), Error> {
        self.sell_buy_internal(amount, batch_id, Operation::Buy)
    }

    pub fn revert_sell(&mut self, amount: u128, batch_id: BatchId) -> Result<(), Error> {
        self.revert_sell_buy_internal(amount, batch_id, Operation::Sell)
    }

    pub fn revert_buy(&mut self, amount: u128, batch_id: BatchId) -> Result<(), Error> {
        self.revert_sell_buy_internal(amount, batch_id, Operation::Buy)
    }

    fn get_balance_internal(
        &self,
        batch_id: BatchId,
        include_withdraw: bool,
    ) -> Result<U256, Error> {
        let mut balance = self.balance;
        if self.deposit.batch_id < batch_id {
            balance = balance
                .checked_add(self.deposit.amount)
                .ok_or(Error::MathOverflow)?;
        }
        // TODO: this could temporarily fail while not all trades for solution have been received.
        if self.pending_solution.batch_id < batch_id {
            balance = balance
                .checked_add(self.pending_solution.increase)
                .ok_or(Error::MathOverflow)?
                .checked_sub(self.pending_solution.decrease)
                .ok_or(Error::MathOverflow)?;
        }
        if include_withdraw && self.withdraw.batch_id < batch_id {
            // Saturating because withdraw requests can be for amounts larger than balance.
            balance = balance.saturating_sub(self.withdraw.amount);
        }
        Ok(balance)
    }

    fn clear_pending_deposit_and_solution_if_possible(&mut self, batch_id: BatchId) {
        if self.deposit.batch_id < batch_id {
            self.deposit.amount = 0.into();
        }
        if self.pending_solution.batch_id < batch_id {
            self.pending_solution.increase = 0.into();
            self.pending_solution.decrease = 0.into();
        }
    }

    fn sell_buy_internal(
        &mut self,
        amount: u128,
        batch_id: BatchId,
        operation: Operation,
    ) -> Result<(), Error> {
        let new_balance = self.get_balance(batch_id)?;
        match self.pending_solution.batch_id.cmp(&batch_id) {
            Ordering::Less => {
                self.balance = new_balance;
                self.clear_pending_deposit_and_solution_if_possible(batch_id);
                self.pending_solution.batch_id = batch_id;
                *self.pending_solution.get_field(operation) = amount.into();
            }
            Ordering::Equal => {
                let field = self.pending_solution.get_field(operation);
                *field = field
                    .checked_add(amount.into())
                    .ok_or(Error::MathOverflow)?
            }
            Ordering::Greater => return Err(Error::TradeForPastBatch),
        }
        Ok(())
    }

    fn revert_sell_buy_internal(
        &mut self,
        amount: u128,
        batch_id: BatchId,
        operation: Operation,
    ) -> Result<(), Error> {
        if self.pending_solution.batch_id != batch_id {
            return Err(Error::RevertingNonExistentTrade);
        }
        let field = match operation {
            Operation::Sell => &mut self.pending_solution.decrease,
            Operation::Buy => &mut self.pending_solution.increase,
        };
        *field = field
            .checked_sub(amount.into())
            .ok_or(Error::MathOverflow)?;
        Ok(())
    }
}
