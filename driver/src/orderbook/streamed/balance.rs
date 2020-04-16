use super::*;
use error::Error;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize)]
pub struct Balance {
    balance: U256,
    pub pending_deposit: Option<Flux>,
    pub pending_withdraw: Option<Flux>,
}

impl Balance {
    pub fn deposit(&mut self, new_deposit: Flux, current_batch_id: BatchId) {
        self.update_deposit_balance(current_batch_id);
        match self.pending_deposit.as_mut() {
            Some(deposit) => {
                deposit.amount += new_deposit.amount;
                deposit.batch_id = new_deposit.batch_id
            }
            None => self.pending_deposit = Some(new_deposit),
        }
    }

    pub fn withdraw(&mut self, amount: U256, current_batch_id: BatchId) -> Result<(), Error> {
        self.update_deposit_balance(current_batch_id);
        self.pending_withdraw = None;
        self.balance = self
            .balance
            .checked_sub(amount)
            .ok_or(Error::MathUnderflow)?;
        Ok(())
    }

    pub fn update_deposit_balance(&mut self, current_batch_id: BatchId) {
        match self.pending_deposit {
            Some(ref deposit) if deposit.batch_id < current_batch_id => {
                self.balance += deposit.amount;
                self.pending_deposit = None;
            }
            _ => (),
        };
    }

    pub fn current_balance(&self, current_batch_id: BatchId) -> U256 {
        let mut balance = self.balance;
        if let Some(ref flux) = self.pending_deposit {
            if flux.batch_id < current_batch_id {
                balance = balance.saturating_add(flux.amount);
            }
        }
        if let Some(ref flux) = self.pending_withdraw {
            if flux.batch_id < current_batch_id {
                balance = balance.saturating_sub(flux.amount);
            }
        }
        balance
    }
}

#[derive(Copy, Clone, Debug, Deserialize, Serialize)]
pub struct Flux {
    pub amount: U256,
    pub batch_id: BatchId,
}
