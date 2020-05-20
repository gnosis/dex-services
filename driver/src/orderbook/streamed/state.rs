use super::*;
use crate::contracts::stablex_contract::batch_exchange::{event_data::*, Event};
use crate::models::Order as ModelOrder;
use anyhow::{anyhow, bail, ensure, Result};
use balance::Balance;
use order::Order;
use serde::{Deserialize, Serialize};
use std::collections::hash_map::Entry;
use std::collections::HashMap;
use std::iter::Iterator;

// Most types, fields, functions in this module mirror the smart contract because we need to
// emulate what it does based on the events it emits.

/// The orderbook state as built from received events.
///
/// Note that there is no way to revert an event. The order in which events are received matters
/// Applying events `A B` does not always result in the same state as `B A`. For example, a Trade
/// can only be processed if the OrderPlacement for the traded order has previously been observed.
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct State {
    orders: HashMap<(UserId, OrderId), Order>,
    balances: HashMap<(UserId, TokenAddress), Balance>,
    tokens: Tokens,
    last_solution: LastSolution,
    /// True when we have received some trades or trade reverts but not yet the final solution
    /// submission that indicates all trades have been received.
    solution_partially_received: bool,
    last_batch_id: BatchId,
}

#[derive(Debug)]
pub enum Batch {
    /// The current completed batch that can no longer change
    Current,
    /// A future potentially still changing batch if a new solution comes in
    Future(BatchId),
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
struct LastSolution {
    batch_id: BatchId,
    user_id: UserId,
    burnt_fees: U256,
}

impl State {
    /// Errors if State has only received a partial solution which would make the result
    /// inconsistent.
    /// Errors if the batch is `Future` but is not actually in the future.
    /// Account balances that overflow a U256 are skipped.
    pub fn orderbook_for_batch(
        &self,
        batch: Batch,
    ) -> Result<(
        impl Iterator<Item = ((UserId, TokenId), U256)> + '_,
        impl Iterator<Item = ModelOrder> + '_,
    )> {
        let batch_id = match batch {
            Batch::Current => self.last_batch_id,
            Batch::Future(batch_id) => {
                // We allow the batch ids being equal to prevent race conditions where the State gets
                // a new event right before we want to get the orderbook.
                ensure!(self.last_batch_id <= batch_id, "batch is in the past");
                // TODO: in the future we might want to handle the case where
                // solution_partially_received is true and react in some way like erroring or
                // excluding pending balances.
                batch_id
            }
        };
        Ok((self.account_state(batch_id), self.orders(batch_id)))
    }

    fn account_state(
        &self,
        batch_id: BatchId,
    ) -> impl Iterator<Item = ((UserId, TokenId), U256)> + '_ {
        self.balances
            .iter()
            .filter_map(move |((user_id, token_address), balance)| {
                // It is possible that a user has a balance for a token that hasn't been added to
                // the exchange because tokens can be deposited anyway.
                let token_id = self.tokens.get_id_by_address(*token_address)?;
                Some((
                    (*user_id, token_id),
                    // Can fail if user's balance exceeds U256::max.
                    // Can fail while not all trades of a solution have been received and batch_id
                    // is the next batch_id so that we assume that the current solution won't be be
                    // reverted.
                    bigint_u256::bigint_to_u256(&balance.get_balance(batch_id))?,
                ))
            })
    }

    fn orders(&self, batch_id: BatchId) -> impl Iterator<Item = ModelOrder> + '_ {
        self.orders
            .iter()
            // State is returned **excluding** the given `batch_id` however order validity is internally stored
            // **including** `batch_id`. Thus we need subtract 1 here to get all orders valid for batch_id -1.
            .filter(move |(_, order)| order.is_valid_in_batch(batch_id - 1))
            .map(move |((user_id, order_id), order)| {
                order.as_model_order(batch_id, *user_id, *order_id)
            })
    }

    /// Reset the state to the default state in which no events have been applied.
    pub fn clear(&mut self) {
        self.orders.clear();
        self.balances.clear();
        self.tokens.0.clear();
    }

    /// Apply an event to the state, modifying it.
    ///
    /// Consumes and returns back `self` because it cannot be reused in case of error.
    ///
    /// `block_batch_id` is the current batch based on the timestamp of the block that contains the
    ///  event.
    pub fn apply_event(mut self, event: &Event, block_batch_id: BatchId) -> Result<Self> {
        ensure!(self.last_batch_id <= block_batch_id, "event in the past");
        self.last_batch_id = block_batch_id;
        match event {
            Event::Deposit(event) => self.deposit(event, block_batch_id)?,
            Event::WithdrawRequest(event) => self.withdraw_request(event, block_batch_id)?,
            Event::Withdraw(event) => self.withdraw(event, block_batch_id)?,
            Event::TokenListing(event) => self.token_listing(event)?,
            Event::OrderPlacement(event) => self.order_placement(event)?,
            Event::OrderCancellation(event) => self.order_cancellation(event, block_batch_id)?,
            Event::OrderDeletion(event) => self.order_deletion(event, block_batch_id)?,
            Event::Trade(event) => self.apply_trade(event, block_batch_id)?,
            Event::TradeReversion(event) => self.apply_trade_reversion(event, block_batch_id)?,
            Event::SolutionSubmission(event) => {
                self.apply_solution_submission(event, block_batch_id)?
            }
        };
        Ok(self)
    }

    fn deposit(&mut self, event: &Deposit, block_batch_id: BatchId) -> Result<()> {
        ensure!(
            event.batch_id == block_batch_id,
            "deposit batch id does not match current batch id"
        );
        let balance = self.balances.entry((event.user, event.token)).or_default();
        balance.deposit(event.amount, event.batch_id);
        Ok(())
    }

    fn withdraw_request(&mut self, event: &WithdrawRequest, block_batch_id: BatchId) -> Result<()> {
        ensure!(
            event.batch_id >= block_batch_id,
            "withdraw request in the past"
        );
        let balance = self.balances.entry((event.user, event.token)).or_default();
        balance.withdraw_request(event.amount, event.batch_id, block_batch_id)
    }

    fn withdraw(&mut self, event: &Withdraw, block_batch_id: BatchId) -> Result<()> {
        let balance = self.balances.entry((event.user, event.token)).or_default();
        balance.withdraw(event.amount, block_batch_id)
    }

    fn token_listing(&mut self, event: &TokenListing) -> Result<()> {
        self.tokens.0.push((event.id, event.token));
        Ok(())
    }

    fn order_placement(&mut self, event: &OrderPlacement) -> Result<()> {
        let order = Order::new(
            event.buy_token,
            event.sell_token,
            event.valid_from,
            event.valid_until,
            event.price_numerator,
            event.price_denominator,
        );
        match self.orders.entry((event.owner, event.index)) {
            Entry::Vacant(entry) => entry.insert(order),
            Entry::Occupied(_) => bail!("order already exists"),
        };
        Ok(())
    }

    fn order_cancellation(
        &mut self,
        event: &OrderCancellation,
        block_batch_id: BatchId,
    ) -> Result<()> {
        let order = self
            .orders
            .get_mut(&(event.owner, event.id))
            .ok_or_else(|| anyhow!("unknown order"))?;
        order.valid_until = block_batch_id - 1;
        Ok(())
    }

    fn order_deletion(&mut self, event: &OrderDeletion, block_batch_id: BatchId) -> Result<()> {
        if let Some(order) = self.orders.get(&(event.owner, event.id)) {
            ensure!(
                !order.is_valid_in_batch(block_batch_id - 1),
                "deleting valid order"
            );
            self.orders.remove(&(event.owner, event.id));
        }
        // Orders are allowed to be deleted multiple times so it is not an error to not find the
        // order.
        Ok(())
    }

    fn apply_trade(&mut self, event: &Trade, block_batch_id: BatchId) -> Result<()> {
        self.apply_trade_internal(
            event.owner,
            event.order_id,
            |order| order.trade(event.executed_sell_amount, block_batch_id),
            |sell_balance| sell_balance.sell(event.executed_sell_amount, block_batch_id),
            |buy_balance| buy_balance.buy(event.executed_buy_amount, block_batch_id),
        )
    }

    fn apply_trade_reversion(
        &mut self,
        event: &TradeReversion,
        block_batch_id: BatchId,
    ) -> Result<()> {
        self.apply_trade_internal(
            event.owner,
            event.order_id,
            |order| order.revert_trade(event.executed_sell_amount, block_batch_id),
            |sell_balance| sell_balance.revert_sell(event.executed_sell_amount, block_batch_id),
            |buy_balance| buy_balance.revert_buy(event.executed_buy_amount, block_batch_id),
        )
    }

    fn apply_trade_internal(
        &mut self,
        user_id: UserId,
        order_id: OrderId,
        order_fn: impl FnOnce(&mut Order) -> Result<()>,
        sell_balance_fn: impl FnOnce(&mut Balance) -> Result<()>,
        buy_balance_fn: impl FnOnce(&mut Balance) -> Result<()>,
    ) -> Result<()> {
        self.solution_partially_received = true;

        let order = self
            .orders
            .get_mut(&(user_id, order_id))
            .ok_or_else(|| anyhow!("unknown order"))?;
        order_fn(order)?;

        let sell_token = self
            .tokens
            .get_address_by_id(order.sell_token)
            .ok_or_else(|| anyhow!("unknown sell token"))?;
        let sell_balance = self.balances.entry((user_id, sell_token)).or_default();
        sell_balance_fn(sell_balance)?;

        let buy_token = self
            .tokens
            .get_address_by_id(order.buy_token)
            .ok_or_else(|| anyhow!("unknown buy token"))?;
        let buy_balance = self.balances.entry((user_id, buy_token)).or_default();
        buy_balance_fn(buy_balance)
    }

    fn apply_solution_submission(
        &mut self,
        event: &SolutionSubmission,
        block_batch_id: BatchId,
    ) -> Result<()> {
        let fee_token = self
            .tokens
            .get_address_by_id(0)
            .ok_or_else(|| anyhow!("solution without fee token"))?;
        self.revert_last_solution(fee_token, block_batch_id);
        self.last_solution.batch_id = block_batch_id;
        self.last_solution.user_id = event.submitter;
        self.last_solution.burnt_fees = event.burnt_fees;
        self.solution_partially_received = false;
        self.balances
            .entry((event.submitter, fee_token))
            .or_default()
            .solution_submission(event.burnt_fees, block_batch_id)
    }

    fn revert_last_solution(&mut self, fee_token: TokenAddress, block_batch_id: BatchId) {
        if self.last_solution.batch_id == block_batch_id {
            // Neither unwrap can fail because we must have previously added the fee to the
            // submitter in which case the balance must exist and be reversible.
            self.balances
                .get_mut(&(self.last_solution.user_id, fee_token))
                .unwrap()
                .revert_solution_submission(self.last_solution.burnt_fees, block_batch_id)
                .unwrap();
            self.last_solution.burnt_fees = U256::zero();
        }
    }
}

/// Bidirectional map between token id and token address.
///
/// Std does not have this type so we use a vector. Alternatively we could find a crate.
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
struct Tokens(Vec<(TokenId, TokenAddress)>);

impl Tokens {
    fn get_address_by_id(&self, id: TokenId) -> Option<Address> {
        self.0
            .iter()
            .find(|(id_, _)| *id_ == id)
            .map(|(_, address)| *address)
    }

    fn get_id_by_address(&self, address: TokenAddress) -> Option<TokenId> {
        self.0
            .iter()
            .find(|(_, address_)| *address_ == address)
            .map(|(token_id, _)| *token_id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::AccountState;

    macro_rules! apply_event {
        (to $state:ident for batch $batch:expr; TokenListing token $token:expr) => {
            let event = TokenListing {
                token: address($token),
                id: $token,
            };
            $state = $state
                .apply_event(&Event::TokenListing(event), $batch)
                .unwrap();
        };
        (to $state:ident for batch $batch:expr; Deposit token $token:expr, to user $user:expr, amount $amount:expr) => {
            let event = Deposit {
                user: address($user),
                token: address($token),
                amount: U256::from($amount),
                batch_id: $batch,
            };
            $state = $state.apply_event(&Event::Deposit(event), $batch).unwrap();
        };
        (to $state:ident for batch $batch:expr; WithdrawRequest from user $user:expr, of token $token:expr, amount $amount:expr, valid after batch $valid_from:expr) => {
            let event = WithdrawRequest {
                user: address($user),
                token: address($token),
                amount: U256::from($amount),
                batch_id: $valid_from,
            };
            $state = $state
                .apply_event(&Event::WithdrawRequest(event), $batch)
                .unwrap();
        };
        (to $state:ident for batch $batch:expr; Withdraw from user $user:expr, of token $token:expr, amount $amount:expr) => {
            let event = Withdraw {
                user: address($user),
                token: address($token),
                amount: U256::from($amount),
            };
            $state = $state.apply_event(&Event::Withdraw(event), $batch).unwrap();
        };
        (to $state:ident for batch $batch:expr; OrderPlacement number $index:expr, from user $user:expr, selling $sell_amount:expr, of token $sell_token:expr, for at least $buy_amount:expr, of token $buy_token:expr, for batch interval [$valid_from:expr, $valid_until:expr]) => {
            let event = OrderPlacement {
                owner: address($user),
                index: $index,
                buy_token: $buy_token,
                sell_token: $sell_token,
                valid_from: $valid_from,
                valid_until: $valid_until,
                price_numerator: $sell_amount,
                price_denominator: $buy_amount,
            };
            $state = $state
                .apply_event(&Event::OrderPlacement(event), $batch)
                .unwrap();
        };
        (to $state:ident for batch $batch:expr; Trade order number $index:expr, from user $user:expr, selling $sell_amount:expr, for $buy_amount:expr) => {
            let event = Trade {
                owner: address($user),
                order_id: $index,
                executed_sell_amount: $sell_amount,
                executed_buy_amount: $buy_amount,
                ..Default::default()
            };
            $state = $state.apply_event(&Event::Trade(event), $batch).unwrap();
        };
        (to $state:ident for batch $batch:expr; TradeReversion order number $index:expr, from user $user:expr, selling $sell_amount:expr, for $buy_amount:expr) => {
            let event = TradeReversion {
                owner: address($user),
                order_id: $index,
                executed_sell_amount: $sell_amount,
                executed_buy_amount: $buy_amount,
                ..Default::default()
            };
            $state = $state
                .apply_event(&Event::TradeReversion(event), $batch)
                .unwrap();
        };
        (to $state:ident for batch $batch:expr; SolutionSubmission from user $user:expr, with fee $fee:expr) => {
            let event = SolutionSubmission {
                submitter: address($user),
                burnt_fees: U256::from($fee),
                ..Default::default()
            };
            $state = $state
                .apply_event(&Event::SolutionSubmission(event), $batch)
                .unwrap();
        };
    }

    macro_rules! assert_balance {
        (in $state:ident for batch $batch:expr; user $user:expr, has token $token:expr, balance $balance:expr) => {
            assert_eq!(
                account_state(&$state, $batch).read_balance($token, address($user)),
                U256::from($balance)
            );
        };
    }

    macro_rules! assert_used_amount {
        (in $state:ident for batch $batch:expr; of order number $index:expr, from user $user:expr, is $amount:expr) => {
            assert_eq!(
                $state
                    .orders
                    .get(&(address($user), $index))
                    .unwrap()
                    .get_used_amount($batch),
                $amount
            );
        };
    }

    fn address(n: u64) -> Address {
        Address::from_low_u64_be(n)
    }

    fn state_with_fee() -> State {
        let mut state = State::default();
        apply_event!(to state for batch 0; TokenListing token 0);
        state
    }

    fn account_state(state: &State, batch_id: BatchId) -> AccountState {
        AccountState(state.account_state(batch_id).collect())
    }

    #[test]
    fn account_state_respects_deposit_batch() {
        let mut state = state_with_fee();
        apply_event!(to state for batch 0; Deposit token 0, to user 3, amount 1);
        assert_balance!(in state for batch 0; user 3, has token 0, balance 0);
        assert_balance!(in state for batch 1; user 3, has token 0, balance 1);
    }

    #[test]
    fn account_state_works_with_unlisted_token() {
        let mut state = state_with_fee();
        // token id 1 is not listed
        apply_event!(to state for batch 0; Deposit token 0, to user 3, amount 1);
        apply_event!(to state for batch 0; Deposit token 1, to user 3, amount 1);
        assert_balance!(in state for batch 1; user 3, has token 0, balance 1);
        assert_balance!(in state for batch 1; user 3, has token 1, balance 0);

        apply_event!(to state for batch 1; TokenListing token 1);
        assert_balance!(in state for batch 1; user 3, has token 0, balance 1);
        assert_balance!(in state for batch 1; user 3, has token 1, balance 1);
    }

    #[test]
    fn multiple_deposits_in_different_batches() {
        let mut state = state_with_fee();
        for i in 0..3 {
            apply_event!(to state for batch i; Deposit token 0, to user 1, amount 1);
            assert_balance!(in state for batch i + 2; user 1, has token 0, balance i + 1);
        }
    }

    #[test]
    fn multiple_deposits_in_same_batch() {
        let mut state = state_with_fee();
        for i in 0..3 {
            apply_event!(to state for batch 0; Deposit token 0, to user 1, amount 1);
            assert_balance!(in state for batch 1; user 1, has token 0, balance i + 1);
        }
    }

    #[test]
    fn withdraw_request_deducted_from_balance() {
        let mut state = state_with_fee();
        apply_event!(to state for batch 0; Deposit token 0, to user 1, amount 2);
        apply_event!(to state for batch 0; WithdrawRequest from user 1, of token 0, amount 1, valid after batch 0);
        assert_balance!(in state for batch 1; user 1, has token 0, balance 1);
    }

    #[test]
    fn withdraw_request_does_not_underflow() {
        let mut state = state_with_fee();
        apply_event!(to state for batch 0; Deposit token 0, to user 1, amount 2);
        apply_event!(to state for batch 1; WithdrawRequest from user 1, of token 0, amount 3, valid after batch 2);
        assert_balance!(in state for batch 3; user 1, has token 0, balance 0);
    }

    #[test]
    fn withdraw_deducted_from_balance() {
        let mut state = state_with_fee();
        apply_event!(to state for batch 0; Deposit token 0, to user 1, amount 2);
        apply_event!(to state for batch 0; WithdrawRequest from user 1, of token 0, amount 2, valid after batch 0);
        apply_event!(to state for batch 1; Withdraw from user 1, of token 0, amount 1);
        assert_balance!(in state for batch 1; user 1, has token 0, balance 1);
    }

    #[test]
    fn withdraw_removes_pending_withdraw() {
        let mut state = state_with_fee();
        apply_event!(to state for batch 0; Deposit token 0, to user 1, amount 3);
        apply_event!(to state for batch 0; WithdrawRequest from user 1, of token 0, amount 2, valid after batch 0);
        assert_balance!(in state for batch 1; user 1, has token 0, balance 1);
        apply_event!(to state for batch 1; Withdraw from user 1, of token 0, amount 1);
        assert_balance!(in state for batch 2; user 1, has token 0, balance 2);
    }

    #[test]
    fn order_placement_cancellation_deletion() {
        let mut state = state_with_fee();
        assert_eq!(state.orders(0).next(), None);
        apply_event!(
            to state for batch 0; OrderPlacement number 0, from user 2,
            selling 3, of token 1, for at least 4, of token 0, for batch interval [1, 2]
        );

        assert_eq!(state.orders(1).next(), None);
        let expected_orders = vec![ModelOrder {
            id: 0,
            account_id: address(2),
            buy_token: 0,
            sell_token: 1,
            buy_amount: 3,
            sell_amount: 4,
        }];
        assert_eq!(state.orders(2).collect::<Vec<_>>(), expected_orders);
        assert_eq!(state.orders(3).collect::<Vec<_>>(), expected_orders);
        assert_eq!(state.orders(4).next(), None);

        let event = OrderCancellation {
            owner: address(2),
            id: 0,
        };
        state = state
            .apply_event(&Event::OrderCancellation(event), 2)
            .unwrap();

        assert_eq!(state.orders(1).next(), None);
        assert_eq!(state.orders(2).collect::<Vec<_>>(), expected_orders);
        assert_eq!(state.orders(3).next(), None);
        assert_eq!(state.orders(4).next(), None);

        let event = Event::OrderDeletion(OrderDeletion {
            owner: address(2),
            id: 0,
        });
        assert!(state.clone().apply_event(&event, 2).is_err());
        state = state.apply_event(&event, 3).unwrap();
        assert_eq!(state.orders(1).next(), None);
        assert_eq!(state.orders(2).next(), None);
        assert_eq!(state.orders(3).next(), None);
        assert_eq!(state.orders(4).next(), None);
    }

    #[test]
    fn trade_and_reversion_and_solution() {
        let mut state = state_with_fee();
        apply_event!(to state for batch 0; TokenListing token 1);

        for token in 0..2 {
            for user in 2..4 {
                apply_event!(to state for batch 0; Deposit token token, to user user, amount 10);
            }
        }

        apply_event!(
            to state for batch 0; OrderPlacement number 0, from user 2,
            selling 5, of token 1, for at least 5, of token 0, for batch interval [0, 10]
        );
        apply_event!(
            to state for batch 0; OrderPlacement number 0, from user 3,
            selling 3, of token 0, for at least 3, of token 1, for batch interval [0, 10]
        );

        apply_event!(to state for batch 1; Trade order number 0, from user 2, selling 1, for 2);
        apply_event!(to state for batch 1; Trade order number 0, from user 3, selling 2, for 1);
        apply_event!(to state for batch 1; SolutionSubmission from user 4, with fee 23);

        assert_balance!(in state for batch 2; user 2, has token 0, balance 12);
        assert_balance!(in state for batch 2; user 2, has token 1, balance 9);
        assert_balance!(in state for batch 2; user 3, has token 0, balance 8);
        assert_balance!(in state for batch 2; user 3, has token 1, balance 11);
        assert_balance!(in state for batch 2; user 4, has token 0, balance 23);
        assert_used_amount!(in state for batch 2; of order number 0, from user 2, is 1);
        assert_used_amount!(in state for batch 2; of order number 0, from user 3, is 2);

        apply_event!(to state for batch 1; TradeReversion order number 0, from user 3, selling 2, for 1);
        apply_event!(to state for batch 1; TradeReversion order number 0, from user 2, selling 1, for 2);
        apply_event!(to state for batch 1; SolutionSubmission from user 4, with fee 42);

        assert_balance!(in state for batch 2; user 2, has token 0, balance 10);
        assert_balance!(in state for batch 2; user 2, has token 1, balance 10);
        assert_balance!(in state for batch 2; user 3, has token 0, balance 10);
        assert_balance!(in state for batch 2; user 3, has token 1, balance 10);
        assert_balance!(in state for batch 2; user 4, has token 0, balance 42);
        assert_used_amount!(in state for batch 2; of order number 0, from user 2, is 0);
        assert_used_amount!(in state for batch 2; of order number 0, from user 3, is 0);
    }

    #[test]
    fn orderbook_batch_id() {
        let mut state = state_with_fee();
        apply_event!(to state for batch 1; Deposit token 0, to user 3, amount 1);
        let balance = state
            .orderbook_for_batch(Batch::Current)
            .unwrap()
            .0
            .next()
            .unwrap()
            .1;
        assert_eq!(balance, U256::zero());
        let balance = state
            .orderbook_for_batch(Batch::Future(2))
            .unwrap()
            .0
            .next()
            .unwrap()
            .1;
        assert_eq!(balance, U256::one());
    }

    #[test]
    fn orderbook_partial_solution() {
        let mut state = state_with_fee();
        apply_event!(to state for batch 0; TokenListing token 1);
        for token in 0..2 {
            apply_event!(to state for batch 0; Deposit token token, to user 2, amount 10);
        }
        apply_event!(
            to state for batch 0; OrderPlacement number 0, from user 2,
            selling 5, of token 1, for at least 5, of token 0, for batch interval [0, 10]
        );
        apply_event!(to state for batch 1; Trade order number 0, from user 2, selling 1, for 2);

        assert_balance!(in state for batch 1; user 2, has token 0, balance 10);
        assert_balance!(in state for batch 1; user 2, has token 1, balance 10);

        apply_event!(to state for batch 1; SolutionSubmission from user 4, with fee 42);

        assert_balance!(in state for batch 2; user 2, has token 0, balance 12);
        assert_balance!(in state for batch 2; user 2, has token 1, balance 9);
    }

    #[test]
    fn orderbook_solution_touching_same_order_twice() {
        let mut state = state_with_fee();
        apply_event!(to state for batch 0; TokenListing token 1);

        apply_event!(to state for batch 0; Deposit token 0, to user 2, amount 20);
        apply_event!(to state for batch 0; Deposit token 1, to user 3, amount 20);

        apply_event!(
            to state for batch 0; OrderPlacement number 0, from user 2,
            selling 10, of token 0, for at least 10, of token 1, for batch interval [0, 10]
        );
        apply_event!(
            to state for batch 0; OrderPlacement number 0, from user 3,
            selling 5, of token 1, for at least 5, of token 0, for batch interval [0, 10]
        );
        apply_event!(
            to state for batch 0; OrderPlacement number 1, from user 3,
            selling 5, of token 1, for at least 5, of token 0, for batch interval [0, 10]
        );

        // the one order by address(2) is split into two trades in the same solution
        apply_event!(to state for batch 1; Trade order number 0, from user 2, selling 4, for 4);
        apply_event!(to state for batch 1; Trade order number 0, from user 2, selling 4, for 4);
        apply_event!(to state for batch 1; Trade order number 0, from user 3, selling 4, for 4);
        apply_event!(to state for batch 1; Trade order number 1, from user 3, selling 4, for 4);
        apply_event!(to state for batch 1; SolutionSubmission from user 4, with fee 42);

        assert_balance!(in state for batch 1; user 2, has token 0, balance 20);
        assert_balance!(in state for batch 1; user 2, has token 1, balance 0);
        assert_balance!(in state for batch 1; user 3, has token 0, balance 0);
        assert_balance!(in state for batch 1; user 3, has token 1, balance 20);

        assert_balance!(in state for batch 2; user 2, has token 0, balance 12);
        assert_balance!(in state for batch 2; user 2, has token 1, balance 8);
        assert_balance!(in state for batch 2; user 3, has token 0, balance 8);
        assert_balance!(in state for batch 2; user 3, has token 1, balance 12);
    }
}
