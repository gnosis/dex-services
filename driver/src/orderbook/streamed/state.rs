use super::*;
use crate::contracts::{
    stablex_auction_element,
    stablex_contract::batch_exchange::{event_data::*, Event},
};
use crate::models::Order as ModelOrder;
use ethcontract::U256;
use serde::{Deserialize, Serialize};
use std::collections::hash_map::Entry;
use std::collections::HashMap;
use std::iter::Iterator;
use thiserror::Error;

// Most types, fields, functions in this module mirror the smart contract because we need to
// emulate what it does based on the events it emits.
//
// We keep track of the pending solution for the batch separately instead of applying the trades
// immediately. This ensures that orders and balances have correctly *not* been updated by the
// pending solution until the solution becomes the real solution.
// This has the side effect of simplifying solution revert logic by allowing us to replace the whole
// pending solution instead of having to revert individual trades.

/// The orderbook state as built from received events.
///
/// Note that there is no way to revert an event. The order in which events are received matters
/// Applying events `A B` does not always result in the same state as `B A`. For example, a Trade
/// can only be processed if the OrderPlacement for the traded order has previously been observed.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct State {
    orders: HashMap<(UserId, OrderId), Order>,
    balances: HashMap<(UserId, TokenAddress), Balance>,
    tokens: Tokens,
    pending_solution: PendingSolution,
}

impl Default for State {
    fn default() -> Self {
        Self {
            orders: HashMap::new(),
            balances: HashMap::new(),
            tokens: Tokens(Vec::new()),
            pending_solution: PendingSolution::AccumulatingTrades(Vec::new()),
        }
    }
}

impl State {
    /// Can be used to create a `crate::models::AccountState`.
    pub fn account_state(
        &self,
        batch_id: BatchId,
    ) -> Result<impl Iterator<Item = ((UserId, TokenId), u128)> + '_, Error> {
        if self.needs_to_apply_solution(batch_id) {
            return Err(Error::NeedsToApplySolution);
        }
        let tokens = &self.tokens;
        let balances =
            self.balances
                .iter()
                .filter_map(move |((user_id, token_address), balance)| {
                    // It is possible that a user has a balance for a token that hasn't been added to
                    // the exchange because tokens can be deposited anyway.
                    let token_id = tokens.get_id_by_address(*token_address)?;
                    Some((
                        (*user_id, token_id),
                        balance.current_balance(batch_id).low_u128(),
                    ))
                });
        Ok(balances)
    }

    pub fn orders(
        &self,
        batch_id: BatchId,
    ) -> Result<impl Iterator<Item = ModelOrder> + '_, Error> {
        if self.needs_to_apply_solution(batch_id) {
            return Err(Error::NeedsToApplySolution);
        }
        let orders = self
            .orders
            .iter()
            .filter(move |(_, order)| order.is_valid_in_batch(batch_id))
            .map(|((user_id, order_id), order)| {
                let (buy_amount, sell_amount) = stablex_auction_element::compute_buy_sell_amounts(
                    order.price_numerator,
                    order.price_denominator,
                    order.price_denominator - order.used_amount,
                );
                ModelOrder {
                    id: *order_id,
                    account_id: *user_id,
                    buy_token: order.buy_token,
                    sell_token: order.sell_token,
                    buy_amount,
                    sell_amount,
                }
            });
        Ok(orders)
    }

    /// Returns whether the state needs to be updated with a pending solution that becomes the
    /// accepted solution because the batch id has incremented. In this case
    /// `apply_pending_solution_if_needed` has to be called before `account_state` and `orders` can
    /// be used.
    pub fn needs_to_apply_solution(&self, batch_id_: BatchId) -> bool {
        match self.pending_solution {
            PendingSolution::Submitted { batch_id, .. } if batch_id < batch_id_ => true,
            _ => false,
        }
    }

    /// Reset the state to the default state in which no events have been applied.
    pub fn clear(&mut self) {
        self.orders.clear();
        self.balances.clear();
        self.tokens.0.clear();
    }

    /// Apply an event to the state, modifying it.
    ///
    /// In case of error no modifications take place.
    ///
    /// `block_batch_id` is the current batch based on the timestamp of the block that contains the
    ///  event.
    pub fn apply_event(&mut self, event: &Event, block_batch_id: BatchId) -> Result<(), Error> {
        self.apply_pending_solution_if_needed(block_batch_id);
        match event {
            Event::Deposit(event) => self.apply_deposit(event, block_batch_id),
            Event::WithdrawRequest(event) => self.apply_withdraw_request(event),
            Event::Withdraw(event) => self.apply_withdraw(event, block_batch_id),
            Event::TokenListing(event) => self.apply_token_listing(event),
            Event::OrderPlacement(event) => self.apply_order_placement(event),
            Event::OrderCancellation(event) => self.apply_order_cancellation(event, block_batch_id),
            Event::OrderDeletion(event) => self.apply_order_deletion(event, block_batch_id),
            Event::Trade(event) => self.apply_trade(event),
            // No need to do anything. The solution will be reverted by the first trade of the next
            // solution.
            Event::TradeReversion(_) => Ok(()),
            Event::SolutionSubmission(event) => {
                self.apply_solution_submission(event, block_batch_id)
            }
        }
    }

    fn apply_deposit(&mut self, event: &Deposit, block_batch_id: BatchId) -> Result<(), Error> {
        let balance = self.balances.entry((event.user, event.token)).or_default();
        balance.deposit(
            Flux {
                amount: event.amount,
                batch_id: event.batch_id,
            },
            block_batch_id,
        );
        Ok(())
    }

    fn apply_withdraw_request(&mut self, event: &WithdrawRequest) -> Result<(), Error> {
        let balance = self.balances.entry((event.user, event.token)).or_default();
        balance.pending_withdraw = Some(Flux {
            amount: event.amount,
            batch_id: event.batch_id,
        });
        Ok(())
    }

    fn apply_withdraw(&mut self, event: &Withdraw, block_batch_id: BatchId) -> Result<(), Error> {
        let balance = self.balances.entry((event.user, event.token)).or_default();
        balance.withdraw(event.amount, block_batch_id)
    }

    fn apply_token_listing(&mut self, event: &TokenListing) -> Result<(), Error> {
        self.tokens.0.push((event.id, event.token));
        Ok(())
    }

    fn apply_order_placement(&mut self, event: &OrderPlacement) -> Result<(), Error> {
        let order = Order {
            buy_token: event.buy_token,
            sell_token: event.sell_token,
            valid_from: event.valid_from,
            valid_until: event.valid_until,
            price_numerator: event.price_numerator,
            price_denominator: event.price_denominator,
            used_amount: 0,
        };
        match self.orders.entry((event.owner, event.index)) {
            Entry::Vacant(entry) => entry.insert(order),
            Entry::Occupied(_) => return Err(Error::OrderAlreadyExists),
        };
        Ok(())
    }

    fn apply_order_cancellation(
        &mut self,
        event: &OrderCancellation,
        block_batch_id: BatchId,
    ) -> Result<(), Error> {
        let order = self
            .orders
            .get_mut(&(event.owner, event.id))
            .ok_or(Error::UnknownOrder(event.id))?;
        order.valid_until = block_batch_id - 1;
        Ok(())
    }

    fn apply_order_deletion(
        &mut self,
        event: &OrderDeletion,
        block_batch_id: BatchId,
    ) -> Result<(), Error> {
        if let Some(order) = self.orders.get(&(event.owner, event.id)) {
            if order.is_valid_in_batch(block_batch_id - 1) {
                return Err(Error::DeletingValidOrder);
            } else {
                self.orders.remove(&(event.owner, event.id));
            }
        }
        // Orders are allowed to be deleted multiple times.
        Ok(())
    }

    fn apply_trade(&mut self, event: &Trade) -> Result<(), Error> {
        for token in &[event.sell_token, event.buy_token] {
            if self.tokens.get_address_by_id(*token).is_none() {
                return Err(Error::UnknownToken(*token));
            }
        }
        if self.orders.get(&(event.owner, event.order_id)).is_none() {
            return Err(Error::UnknownOrder(event.order_id));
        }
        match &mut self.pending_solution {
            PendingSolution::AccumulatingTrades(trades) => trades.push(event.clone()),
            // If there has been a solution before and we get a new trade then the previous solution
            // must have been reverted.
            PendingSolution::Submitted { .. } => {
                self.pending_solution = PendingSolution::AccumulatingTrades(vec![event.clone()])
            }
        };
        Ok(())
    }

    fn apply_solution_submission(
        &mut self,
        event: &SolutionSubmission,
        block_batch_id: BatchId,
    ) -> Result<(), Error> {
        if self.tokens.get_address_by_id(0).is_none() {
            return Err(Error::SolutionWithoutFeeToken);
        }
        let trades = match &mut self.pending_solution {
            PendingSolution::AccumulatingTrades(trades) => std::mem::replace(trades, Vec::new()),
            // This would be weird because it means that the previous solution had no trades. Not
            // sure if we should error.
            PendingSolution::Submitted { .. } => Vec::new(),
        };
        self.pending_solution = PendingSolution::Submitted {
            batch_id: block_batch_id,
            submitter: event.submitter,
            burnt_fees: event.burnt_fees,
            trades,
        };
        Ok(())
    }

    pub fn apply_pending_solution_if_needed(&mut self, block_batch_id: BatchId) {
        let trades = match &mut self.pending_solution {
            PendingSolution::Submitted {
                batch_id,
                submitter,
                burnt_fees,
                trades,
            } if *batch_id < block_batch_id => {
                // Cannot fail because we ensure there is a fee token when a solution event is
                // received.
                let fee_token = self.tokens.get_address_by_id(0).unwrap();
                let balance = self.balances.entry((*submitter, fee_token)).or_default();
                balance.balance += *burnt_fees;
                // This looks weird but we need to call self.apply_trade outside of the match to
                // prevent multiple mut self borrows.
                let trades = std::mem::replace(trades, Vec::new());
                self.pending_solution = PendingSolution::AccumulatingTrades(Vec::new());
                trades
            }
            _ => return,
        };
        for trade in trades {
            // Cannot fail because we ensure that the tokens exist when a trade event is
            // received and that the order exists and doesn't get removed while it is valid.
            self.apply_solution_trade(
                trade.owner,
                trade.order_id,
                trade.executed_sell_amount,
                trade.executed_buy_amount,
                block_batch_id,
            )
            .unwrap();
        }
    }

    /// Fails if any tokens or order don't exist.
    fn apply_solution_trade(
        &mut self,
        user: UserId,
        order: OrderId,
        executed_sell_amount: u128,
        executed_buy_amount: u128,
        batch_id: BatchId,
    ) -> Result<(), Error> {
        let order = self
            .orders
            .get_mut(&(user, order))
            .ok_or(Error::UnknownOrder(order))?;
        let sell_token = self
            .tokens
            .get_address_by_id(order.sell_token)
            .ok_or(Error::UnknownToken(order.sell_token))?;
        let buy_token = self
            .tokens
            .get_address_by_id(order.buy_token)
            .ok_or(Error::UnknownToken(order.buy_token))?;

        if order.has_limited_amount() {
            order.used_amount += executed_sell_amount;
        }
        self.apply_balance_from_trade(user, sell_token, executed_sell_amount, batch_id, true);
        self.apply_balance_from_trade(user, buy_token, executed_buy_amount, batch_id, false);
        Ok(())
    }

    fn apply_balance_from_trade(
        &mut self,
        user: UserId,
        token: TokenAddress,
        executed_amount: u128,
        batch_id: BatchId,
        subtract: bool,
    ) {
        let balance = self.balances.entry((user, token)).or_default();
        balance.update_deposit_balance(batch_id);
        // It possible that we get an underflow here because the user did not have enough
        // deposited for the sale but in the same batch filled another order that increased their
        // balance but whose trade event we have not yet processed.
        balance.balance = if subtract {
            balance.balance.overflowing_sub(executed_amount.into()).0
        } else {
            balance.balance.overflowing_add(executed_amount.into()).0
        };
    }
}

#[derive(Clone, Copy, Debug, Error)]
pub enum Error {
    #[error("unknown token {0}")]
    UnknownToken(TokenId),
    #[error("unknown order {0}")]
    UnknownOrder(OrderId),
    #[error("order already exists")]
    OrderAlreadyExists,
    #[error("math underflow")]
    MathUnderflow,
    #[error("solution submitted but there is no fee token")]
    SolutionWithoutFeeToken,
    #[error("attempt to delete an order that is still valid")]
    DeletingValidOrder,
    #[error("getting orders or account state failed because there is a pending solution that needs to beapplied")]
    NeedsToApplySolution,
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

#[derive(Copy, Clone, Debug, Deserialize, Serialize)]
struct Order {
    buy_token: TokenId,
    sell_token: TokenId,
    valid_from: BatchId,
    valid_until: BatchId,
    price_numerator: u128,
    price_denominator: u128,
    used_amount: u128,
}

impl Order {
    fn has_limited_amount(&self) -> bool {
        self.price_numerator != std::u128::MAX && self.price_denominator != std::u128::MAX
    }

    fn is_valid_in_batch(&self, batch_id: BatchId) -> bool {
        self.valid_from <= batch_id && batch_id <= self.valid_until
    }
}

#[derive(Copy, Clone, Debug, Deserialize, Serialize)]
struct Flux {
    amount: U256,
    batch_id: BatchId,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize)]
struct Balance {
    balance: U256,
    pending_deposit: Option<Flux>,
    pending_withdraw: Option<Flux>,
}

impl Balance {
    fn deposit(&mut self, new_deposit: Flux, current_batch_id: BatchId) {
        self.update_deposit_balance(current_batch_id);
        match self.pending_deposit.as_mut() {
            Some(deposit) => {
                deposit.amount += new_deposit.amount;
                deposit.batch_id = new_deposit.batch_id
            }
            None => self.pending_deposit = Some(new_deposit),
        }
    }

    fn withdraw(&mut self, amount: U256, current_batch_id: BatchId) -> Result<(), Error> {
        self.update_deposit_balance(current_batch_id);
        self.pending_withdraw = None;
        self.balance = self
            .balance
            .checked_sub(amount)
            .ok_or(Error::MathUnderflow)?;
        Ok(())
    }

    fn update_deposit_balance(&mut self, current_batch_id: BatchId) {
        match self.pending_deposit {
            Some(ref deposit) if deposit.batch_id < current_batch_id => {
                self.balance += deposit.amount;
                self.pending_deposit = None;
            }
            _ => (),
        };
    }

    fn current_balance(&self, current_batch_id: BatchId) -> U256 {
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

/// The current solution for the batch but not yet finale as it can still be replaced by a better
/// solution.
#[derive(Clone, Debug, Deserialize, Serialize)]
enum PendingSolution {
    /// We observed Trade events but no SolutionSubmission.
    AccumulatingTrades(Vec<Trade>),
    /// We observed SolutionSubmission.
    Submitted {
        batch_id: BatchId,
        submitter: UserId,
        burnt_fees: U256,
        trades: Vec<Trade>,
    },
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::AccountState;

    fn address(n: u64) -> Address {
        Address::from_low_u64_be(n)
    }

    fn state_with_fee() -> State {
        let mut state = State::default();
        let event = TokenListing {
            token: address(0),
            id: 0,
        };
        state.apply_event(&Event::TokenListing(event), 0).unwrap();
        state
    }

    fn account_state(state: &mut State, batch_id: BatchId) -> AccountState {
        AccountState(state.account_state(batch_id).unwrap().collect())
    }

    #[test]
    fn account_state_respects_deposit_batch() {
        let mut state = state_with_fee();
        let event = Deposit {
            user: address(3),
            token: address(0),
            amount: 1.into(),
            batch_id: 0,
        };
        state.apply_event(&Event::Deposit(event), 0).unwrap();
        let account_state_ = account_state(&mut state, 0);
        assert_eq!(account_state_.read_balance(0, address(3)), 0);
        let account_state_ = account_state(&mut state, 1);
        assert_eq!(account_state_.read_balance(0, address(3)), 1);
    }

    #[test]
    fn account_state_works_with_unlisted_token() {
        let mut state = state_with_fee();
        // token id 1 is not listed
        for token_id in 0..2 {
            let event = Deposit {
                user: address(3),
                token: address(token_id),
                amount: 1.into(),
                batch_id: 0,
            };
            state.apply_event(&Event::Deposit(event), 0).unwrap();
        }

        let account_state_ = account_state(&mut state, 1);
        assert_eq!(account_state_.read_balance(0, address(3)), 1);

        let event = TokenListing {
            token: address(1),
            id: 1,
        };
        state.apply_event(&Event::TokenListing(event), 0).unwrap();

        let account_state_ = account_state(&mut state, 1);
        assert_eq!(account_state_.read_balance(0, address(3)), 1);
        assert_eq!(account_state_.read_balance(1, address(3)), 1);
    }

    #[test]
    fn multiple_deposits_in_different_batches() {
        let mut state = state_with_fee();
        for i in 0..3 {
            let event = Deposit {
                user: address(1),
                token: address(0),
                amount: 1.into(),
                batch_id: i + 1,
            };
            state.apply_event(&Event::Deposit(event), i).unwrap();

            let account_state_ = account_state(&mut state, i + 2);
            assert_eq!(account_state_.read_balance(0, address(1)), i as u128 + 1);
        }
    }

    #[test]
    fn multiple_deposits_in_same_batch() {
        let mut state = state_with_fee();
        for i in 0..3 {
            let event = Deposit {
                user: address(1),
                token: address(0),
                amount: 1.into(),
                batch_id: 1,
            };
            state.apply_event(&Event::Deposit(event), 0).unwrap();

            let account_state_ = account_state(&mut state, 1);
            assert_eq!(account_state_.read_balance(0, address(1)), 0);
            let account_state_ = account_state(&mut state, 2);
            assert_eq!(account_state_.read_balance(0, address(1)), i + 1);
        }
    }

    #[test]
    fn withdraw_request_deducted_from_balance() {
        let mut state = state_with_fee();
        let event = Deposit {
            user: address(1),
            token: address(0),
            amount: 2.into(),
            batch_id: 0,
        };
        state.apply_event(&Event::Deposit(event), 0).unwrap();
        let event = WithdrawRequest {
            user: address(1),
            token: address(0),
            amount: 1.into(),
            batch_id: 0,
        };
        state
            .apply_event(&Event::WithdrawRequest(event), 0)
            .unwrap();
        let account_state_ = account_state(&mut state, 1);
        assert_eq!(account_state_.read_balance(0, address(1)), 1);
    }

    #[test]
    fn withdraw_request_does_not_underflow() {
        let mut state = state_with_fee();
        let event = Deposit {
            user: address(1),
            token: address(0),
            amount: 2.into(),
            batch_id: 0,
        };
        state.apply_event(&Event::Deposit(event), 0).unwrap();
        let event = WithdrawRequest {
            user: address(1),
            token: address(0),
            amount: 3.into(),
            batch_id: 2,
        };
        state
            .apply_event(&Event::WithdrawRequest(event), 1)
            .unwrap();
        let account_state_ = account_state(&mut state, 3);
        assert_eq!(account_state_.read_balance(0, address(1)), 0);
    }

    #[test]
    fn withdraw_deducted_from_balance() {
        let mut state = state_with_fee();
        let event = Deposit {
            user: address(1),
            token: address(0),
            amount: 2.into(),
            batch_id: 0,
        };
        state.apply_event(&Event::Deposit(event), 0).unwrap();
        let event = Withdraw {
            user: address(1),
            token: address(0),
            amount: 1.into(),
        };
        state.apply_event(&Event::Withdraw(event), 1).unwrap();
        let account_state_ = account_state(&mut state, 2);
        assert_eq!(account_state_.read_balance(0, address(1)), 1);
    }

    #[test]
    fn withdraw_removes_pending_withdraw() {
        let mut state = state_with_fee();
        let event = Deposit {
            user: address(1),
            token: address(0),
            amount: 3.into(),
            batch_id: 0,
        };
        state.apply_event(&Event::Deposit(event), 0).unwrap();
        let event = WithdrawRequest {
            user: address(1),
            token: address(0),
            amount: 1.into(),
            batch_id: 1,
        };
        state
            .apply_event(&Event::WithdrawRequest(event), 1)
            .unwrap();
        let event = Withdraw {
            user: address(1),
            token: address(0),
            amount: 2.into(),
        };
        state.apply_event(&Event::Withdraw(event), 1).unwrap();
        let account_state_ = account_state(&mut state, 2);
        // If the pending withdraw was still active the balance would be 0.
        assert_eq!(account_state_.read_balance(0, address(1)), 1);
    }

    #[test]
    fn order_placement_cancellation_deletion() {
        let mut state = state_with_fee();
        assert_eq!(state.orders(0).unwrap().next(), None);
        let event = OrderPlacement {
            owner: address(2),
            index: 0,
            buy_token: 0,
            sell_token: 1,
            valid_from: 1,
            valid_until: 2,
            price_numerator: 3,
            price_denominator: 4,
        };
        state.apply_event(&Event::OrderPlacement(event), 0).unwrap();

        assert_eq!(state.orders(0).unwrap().next(), None);
        let expected_orders = vec![ModelOrder {
            id: 0,
            account_id: address(2),
            buy_token: 0,
            sell_token: 1,
            buy_amount: 3,
            sell_amount: 4,
        }];
        assert_eq!(
            state.orders(1).unwrap().collect::<Vec<_>>(),
            expected_orders
        );
        assert_eq!(
            state.orders(2).unwrap().collect::<Vec<_>>(),
            expected_orders
        );
        assert_eq!(state.orders(3).unwrap().next(), None);

        let event = OrderCancellation {
            owner: address(2),
            id: 0,
        };
        state
            .apply_event(&Event::OrderCancellation(event), 2)
            .unwrap();

        assert_eq!(state.orders(0).unwrap().next(), None);
        assert_eq!(
            state.orders(1).unwrap().collect::<Vec<_>>(),
            expected_orders
        );
        assert_eq!(state.orders(2).unwrap().next(), None);
        assert_eq!(state.orders(3).unwrap().next(), None);

        let event = Event::OrderDeletion(OrderDeletion {
            owner: address(2),
            id: 0,
        });
        assert!(state.apply_event(&event, 2).is_err());
        state.apply_event(&event, 3).unwrap();
        assert_eq!(state.orders(0).unwrap().next(), None);
        assert_eq!(state.orders(1).unwrap().next(), None);
        assert_eq!(state.orders(2).unwrap().next(), None);
        assert_eq!(state.orders(3).unwrap().next(), None);
    }

    #[test]
    fn trade_and_reversion() {
        let mut state = state_with_fee();
        let event = TokenListing {
            token: address(1),
            id: 1,
        };
        state.apply_event(&Event::TokenListing(event), 0).unwrap();

        for token in 0..2 {
            for user in 2..4 {
                let event = Deposit {
                    user: address(user),
                    token: address(token),
                    amount: 10.into(),
                    batch_id: 0,
                };
                state.apply_event(&Event::Deposit(event), 0).unwrap();
            }
        }

        let event = OrderPlacement {
            owner: address(2),
            index: 0,
            buy_token: 0,
            sell_token: 1,
            valid_from: 0,
            valid_until: 10,
            price_numerator: 5,
            price_denominator: 5,
        };
        state.apply_event(&Event::OrderPlacement(event), 0).unwrap();
        let event = OrderPlacement {
            owner: address(3),
            index: 0,
            buy_token: 1,
            sell_token: 0,
            valid_from: 0,
            valid_until: 10,
            price_numerator: 3,
            price_denominator: 3,
        };
        state.apply_event(&Event::OrderPlacement(event), 0).unwrap();

        let event = Trade {
            owner: address(2),
            order_id: 0,
            executed_sell_amount: 1,
            executed_buy_amount: 2,
            ..Default::default()
        };
        state.apply_event(&Event::Trade(event), 1).unwrap();
        let event = Trade {
            owner: address(3),
            order_id: 0,
            executed_sell_amount: 2,
            executed_buy_amount: 1,
            ..Default::default()
        };
        state.apply_event(&Event::Trade(event), 1).unwrap();
        state
            .apply_event(&Event::SolutionSubmission(SolutionSubmission::default()), 1)
            .unwrap();
        state.apply_pending_solution_if_needed(2);

        let account_state_ = account_state(&mut state, 2);
        assert_eq!(account_state_.read_balance(0, address(2)), 12);
        assert_eq!(account_state_.read_balance(1, address(2)), 9);
        assert_eq!(account_state_.read_balance(0, address(3)), 8);
        assert_eq!(account_state_.read_balance(1, address(3)), 11);
        assert_eq!(state.orders.get(&(address(2), 0)).unwrap().used_amount, 1);
        assert_eq!(state.orders.get(&(address(3), 0)).unwrap().used_amount, 2);
    }

    #[test]
    fn solution_submission_fee() {
        let mut state = state_with_fee();
        let event = SolutionSubmission {
            submitter: address(1),
            burnt_fees: 1.into(),
            ..Default::default()
        };
        state
            .apply_event(&Event::SolutionSubmission(event), 0)
            .unwrap();
        assert!(!state.needs_to_apply_solution(0));
        assert!(state.needs_to_apply_solution(1));
        state.apply_pending_solution_if_needed(1);

        let account_state_ = account_state(&mut state, 1);
        assert_eq!(account_state_.read_balance(0, address(1)), 1);
    }
}
