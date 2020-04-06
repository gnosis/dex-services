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
use std::fmt;
use std::iter::Iterator;

// TODO: Should we handle https://github.com/gnosis/dex-contracts/issues/620 ?
// There is a workaround detailed in the issue it hasn't been implemented.

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
}

impl State {
    /// Can be used to create a `crate::models::AccountState`.
    pub fn account_state(
        &self,
        batch_id: BatchId,
    ) -> impl Iterator<Item = ((UserId, TokenId), u128)> + '_ {
        self.balances
            .iter()
            .filter_map(move |((user_id, token_address), balance)| {
                // It is possible that a user has a balance for a token that hasn't been added to
                // the exchange because tokens can be deposited anyway.
                let token_id = self.tokens.get_id_by_address(*token_address)?;
                Some((
                    (*user_id, token_id),
                    balance.current_balance(batch_id).low_u128(),
                ))
            })
    }

    pub fn orders(&self, batch_id: BatchId) -> impl Iterator<Item = ModelOrder> + '_ {
        self.orders
            .iter()
            .filter(move |(_, order)| order.valid_from <= batch_id && batch_id <= order.valid_until)
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
    /// In case of error no modifications take place.
    ///
    /// `block_batch_id` is the current batch based on the timestamp of the block that contains the
    ///  event.
    pub fn apply_event(&mut self, event: &Event, block_batch_id: BatchId) -> Result<(), Error> {
        match event {
            Event::Deposit(event) => self.deposit(event, block_batch_id),
            Event::WithdrawRequest(event) => self.withdraw_request(event),
            Event::Withdraw(event) => self.withdraw(event, block_batch_id),
            Event::TokenListing(event) => self.token_listing(event),
            Event::OrderPlacement(event) => self.order_placement(event),
            Event::OrderCancellation(event) => self.order_cancellation(event, block_batch_id),
            Event::OrderDeletion(event) => self.order_deletion(event),
            Event::Trade(_event) => unimplemented!(),
            Event::TradeReversion(_event) => unimplemented!(),
            Event::SolutionSubmission(_event) => unimplemented!(),
        }
    }

    fn deposit(&mut self, event: &Deposit, block_batch_id: BatchId) -> Result<(), Error> {
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

    fn withdraw_request(&mut self, event: &WithdrawRequest) -> Result<(), Error> {
        let balance = self.balances.entry((event.user, event.token)).or_default();
        balance.pending_withdraw = Some(Flux {
            amount: event.amount,
            batch_id: event.batch_id,
        });
        Ok(())
    }

    fn withdraw(&mut self, event: &Withdraw, block_batch_id: BatchId) -> Result<(), Error> {
        let balance = self.balances.entry((event.user, event.token)).or_default();
        balance.withdraw(event.amount, block_batch_id)
    }

    fn token_listing(&mut self, event: &TokenListing) -> Result<(), Error> {
        self.tokens.0.push((event.id, event.token));
        Ok(())
    }

    fn order_placement(&mut self, event: &OrderPlacement) -> Result<(), Error> {
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

    fn order_cancellation(
        &mut self,
        event: &OrderCancellation,
        block_batch_id: BatchId,
    ) -> Result<(), Error> {
        let order = self
            .orders
            .get_mut(&(event.owner, event.id))
            .ok_or(Error::UnknownOrder)?;
        order.valid_until = block_batch_id - 1;
        Ok(())
    }

    fn order_deletion(&mut self, event: &OrderDeletion) -> Result<(), Error> {
        // Orders are allowed to be deleted multiple times.
        self.orders.remove(&(event.owner, event.id));
        Ok(())
    }
}

#[derive(Clone, Copy, Debug)]
pub enum Error {
    UnknownToken,
    UnknownOrder,
    OrderAlreadyExists,
    MathUnderflow,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}",
            match self {
                Error::UnknownToken => "unknown token",
                Error::UnknownOrder => "unknown order",
                Error::OrderAlreadyExists => "order already exists",
                Error::MathUnderflow => "math underflow",
            }
        )
    }
}

impl std::error::Error for Error {}

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

    fn account_state(state: &State, batch_id: BatchId) -> AccountState {
        AccountState(state.account_state(batch_id).collect())
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
        let account_state_ = account_state(&state, 0);
        assert_eq!(account_state_.read_balance(0, address(3)), 0);
        let account_state_ = account_state(&state, 1);
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

        let account_state_ = account_state(&state, 1);
        assert_eq!(account_state_.read_balance(0, address(3)), 1);

        let event = TokenListing {
            token: address(1),
            id: 1,
        };
        state.apply_event(&Event::TokenListing(event), 0).unwrap();

        let account_state_ = account_state(&state, 1);
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

            let account_state_ = account_state(&state, i + 2);
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

            let account_state_ = account_state(&state, 1);
            assert_eq!(account_state_.read_balance(0, address(1)), 0);
            let account_state_ = account_state(&state, 2);
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
        let account_state_ = account_state(&state, 1);
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
        let account_state_ = account_state(&state, 3);
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
        let account_state_ = account_state(&state, 2);
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
        let account_state_ = account_state(&state, 2);
        // If the pending withdraw was still active the balance would be 0.
        assert_eq!(account_state_.read_balance(0, address(1)), 1);
    }

    #[test]
    fn order_placement_cancellation_deletion() {
        let mut state = state_with_fee();
        assert_eq!(state.orders(0).next(), None);
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

        assert_eq!(state.orders(0).next(), None);
        let expected_orders = vec![ModelOrder {
            id: 0,
            account_id: address(2),
            buy_token: 0,
            sell_token: 1,
            buy_amount: 3,
            sell_amount: 4,
        }];
        assert_eq!(state.orders(1).collect::<Vec<_>>(), expected_orders);
        assert_eq!(state.orders(2).collect::<Vec<_>>(), expected_orders);
        assert_eq!(state.orders(3).next(), None);

        let event = OrderCancellation {
            owner: address(2),
            id: 0,
        };
        state
            .apply_event(&Event::OrderCancellation(event), 2)
            .unwrap();

        assert_eq!(state.orders(0).next(), None);
        assert_eq!(state.orders(1).collect::<Vec<_>>(), expected_orders);
        assert_eq!(state.orders(2).next(), None);
        assert_eq!(state.orders(3).next(), None);

        let event = OrderDeletion {
            owner: address(2),
            id: 0,
        };
        state.apply_event(&Event::OrderDeletion(event), 2).unwrap();
        assert_eq!(state.orders(0).next(), None);
        assert_eq!(state.orders(1).next(), None);
        assert_eq!(state.orders(2).next(), None);
        assert_eq!(state.orders(3).next(), None);
    }
}
