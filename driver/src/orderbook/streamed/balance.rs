use super::*;
use anyhow::{anyhow, ensure, Result};
use num::BigInt;
use num::Zero as _;
use serde::{Deserialize, Serialize};

/// The balance of a token for a user.
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct Balance {
    balance: BigInt,
    deposit: Flux,
    withdraw: Flux,
    proceeds: Flux,
}

impl Balance {
    pub fn deposit(&mut self, amount: U256, batch_id: BatchId) {
        self.apply_existing_deposit_and_proceeds(batch_id);
        // Works like in the smart contract: If there is an existing deposit we override the
        // batch id and add to the amount.
        self.deposit.batch_id = batch_id;
        self.deposit.amount += bigint_u256::u256_to_bigint(amount);
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
            self.withdraw.batch_id >= current_batch_id || self.withdraw.amount.is_zero(),
            "new withdraw request before clearing of previous withdraw request"
        );
        self.withdraw.batch_id = batch_id;
        self.withdraw.amount = bigint_u256::u256_to_bigint(amount);
        Ok(())
    }

    pub fn withdraw(&mut self, amount: U256, batch_id: BatchId) -> Result<()> {
        let amount = bigint_u256::u256_to_bigint(amount);
        // Works like in the smart contract: Any withdraw unconditionally removes the withdraw
        // request even if the amount is smaller than requested.
        ensure!(
            self.withdraw.amount_and_zero(batch_id) >= amount,
            anyhow!("withdraw does not match withdraw request")
        );
        self.apply_existing_deposit_and_proceeds(batch_id);
        self.balance -= amount;
        Ok(())
    }

    pub fn get_balance(&self, batch_id: BatchId) -> BigInt {
        let balance = self.balance_with_deposit_and_proceeds(batch_id);
        // Withdraw requests can be for amounts larger than balance.
        match self.withdraw.amount(batch_id) {
            Some(amount) if amount < &balance => balance - amount,
            Some(_) => BigInt::zero(),
            None => balance,
        }
    }

    pub fn sell(&mut self, amount: u128, batch_id: BatchId) -> Result<()> {
        self.sell_buy_internal(-BigInt::from(amount), batch_id)
    }

    pub fn buy(&mut self, amount: u128, batch_id: BatchId) -> Result<()> {
        self.sell_buy_internal(BigInt::from(amount), batch_id)
    }

    pub fn revert_sell(&mut self, amount: u128, batch_id: BatchId) -> Result<()> {
        self.revert_sell_buy_internal(-BigInt::from(amount), batch_id)
    }

    pub fn revert_buy(&mut self, amount: u128, batch_id: BatchId) -> Result<()> {
        self.revert_sell_buy_internal(BigInt::from(amount), batch_id)
    }

    pub fn solution_submission(&mut self, fee: U256, batch_id: BatchId) -> Result<()> {
        // We are reusing the `Proceeds` machinery here because the extra balance from a solution
        // submission behaves the same way.
        self.sell_buy_internal(bigint_u256::u256_to_bigint(fee), batch_id)
    }

    pub fn revert_solution_submission(&mut self, fee: U256, batch_id: BatchId) -> Result<()> {
        self.revert_sell_buy_internal(bigint_u256::u256_to_bigint(fee), batch_id)
    }

    /// Adds the amount to the proceeds.
    fn sell_buy_internal(&mut self, amount: BigInt, batch_id: BatchId) -> Result<()> {
        ensure!(self.proceeds.batch_id <= batch_id, "trade for past batch");
        self.apply_existing_deposit_and_proceeds(batch_id);
        // Now old proceeds have been cleared. If there is an existing proceed for this batch then
        // setting the batch_id does nothing and we add to the field.
        self.proceeds.batch_id = batch_id;
        self.proceeds.amount += amount;
        Ok(())
    }

    /// Subtracts the amount from the proceeds.
    fn revert_sell_buy_internal(&mut self, amount: BigInt, batch_id: BatchId) -> Result<()> {
        ensure!(
            self.proceeds.batch_id == batch_id,
            "reverting non existent trade"
        );
        self.proceeds.amount -= amount;
        Ok(())
    }

    fn balance_with_deposit_and_proceeds(&self, current_batch_id: BatchId) -> BigInt {
        let mut result = self.balance.clone();
        if let Some(deposit) = self.deposit.amount(current_batch_id) {
            result += deposit
        }
        if let Some(proceed) = self.proceeds.amount(current_batch_id) {
            result += proceed
        }
        result
    }

    fn apply_existing_deposit_and_proceeds(&mut self, current_batch_id: BatchId) {
        self.balance += self.deposit.amount_and_zero(current_batch_id);
        self.balance += self.proceeds.amount_and_zero(current_batch_id);
    }
}

/// A change in balance starting at some batch id.
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
struct Flux {
    batch_id: BatchId,
    amount: BigInt,
}

impl Flux {
    /// Returns the amount if the batch id is smaller than the self.batch_id, None otherwise.
    fn amount(&self, batch_id: BatchId) -> Option<&BigInt> {
        if self.batch_id < batch_id {
            Some(&self.amount)
        } else {
            None
        }
    }

    /// If self.batch_id is smaller than batch_id, replaces self.amount with 0 and returns the
    /// original. Returns 0 without replacing otherwise.
    fn amount_and_zero(&mut self, batch_id: BatchId) -> BigInt {
        if self.batch_id < batch_id {
            std::mem::replace(&mut self.amount, BigInt::zero())
        } else {
            BigInt::zero()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_balance() {
        let overdrawn_request = Balance {
            balance: BigInt::from(2),
            deposit: Flux::default(),
            withdraw: Flux {
                batch_id: 1,
                amount: BigInt::from(3),
            },
            proceeds: Flux::default(),
        };
        assert_eq!(overdrawn_request.get_balance(0), BigInt::from(2));
        assert_eq!(overdrawn_request.get_balance(1), BigInt::from(2));
        assert_eq!(overdrawn_request.get_balance(2), BigInt::zero());

        let standard_request = Balance {
            balance: BigInt::from(2),
            deposit: Flux::default(),
            withdraw: Flux {
                batch_id: 1,
                amount: BigInt::from(1),
            },
            proceeds: Flux::default(),
        };
        assert_eq!(standard_request.get_balance(0), BigInt::from(2));
        assert_eq!(standard_request.get_balance(1), BigInt::from(2));
        assert_eq!(standard_request.get_balance(2), BigInt::from(1));

        let no_request = Balance {
            balance: BigInt::from(1),
            deposit: Flux::default(),
            withdraw: Flux::default(),
            proceeds: Flux::default(),
        };
        assert_eq!(no_request.get_balance(0), BigInt::from(1));
        assert_eq!(no_request.get_balance(1), BigInt::from(1));
        assert_eq!(no_request.get_balance(2), BigInt::from(1));
    }
}
