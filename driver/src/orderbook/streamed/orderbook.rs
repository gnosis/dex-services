use super::*;
use crate::{
    contracts::stablex_contract::batch_exchange,
    models::{AccountState, Order},
    orderbook::StableXOrderBookReading,
};
use anyhow::Result;
use ethcontract::{contract::EventData, H256, U256};
use state::{Batch, State};
use std::collections::BTreeMap;

// Ethereum events (logs) can be both created and removed. Removals happen if the chain reorganizes
// and ends up not including block that was previously thought to be part of the chain.
// However, the orderbook state (`State`) cannot remove events. To support this, we keep an ordered
// list of all events based on which the state is built.

#[derive(Debug, Eq, Ord, PartialEq, PartialOrd)]
struct EventSortKey {
    block_number: u64,
    /// Is included to differentiate events from the same block number but different blocks which
    /// can happen during reorgs.
    block_hash: H256,
    log_index: usize,
}

#[derive(Debug)]
struct Value {
    event: batch_exchange::Event,
    /// The batch id is calculated based on the timestamp of the block.
    batch_id: BatchId,
}

#[derive(Debug, Default)]
pub struct Orderbook {
    events: BTreeMap<EventSortKey, Value>,
}

impl Orderbook {
    pub fn handle_event_data(
        &mut self,
        event_data: EventData<batch_exchange::Event>,
        block_number: u64,
        log_index: usize,
        block_hash: H256,
        block_timestamp: u64,
    ) {
        let batch_id = block_timestamp as BatchId / 300;
        let key = EventSortKey {
            block_number,
            block_hash,
            log_index,
        };
        match event_data {
            EventData::Added(event) => self.events.insert(key, Value { event, batch_id }),
            EventData::Removed(_event) => self.events.remove(&key),
        };
    }

    fn create_state(&self) -> Result<State> {
        self.events
            .iter()
            .try_fold(State::default(), |state, (_key, value)| {
                state.apply_event(&value.event, value.batch_id)
            })
    }
}

fn filter_account_state(
    account_states: impl Iterator<Item = ((UserId, TokenId), U256)>,
    orders: &[Order],
) -> AccountState {
    let account_states = account_states
        .filter(|((user, token), _)| {
            orders
                .iter()
                .any(|order| order.account_id == *user && order.sell_token == *token)
        })
        // TODO: change AccountState to use U256
        .map(|(key, value)| (key, value.low_u128()))
        .collect();
    AccountState(account_states)
}

impl StableXOrderBookReading for Orderbook {
    fn get_auction_data(&self, index: U256) -> Result<(AccountState, Vec<Order>)> {
        // TODO: Handle future batch ids for when we want to do optimistic solving.
        let state = self.create_state()?;
        let (account_state, orders) = state.orderbook_for_batch(Batch::Future(index.low_u32()))?;
        let orders = orders.collect::<Vec<_>>();
        let account_state = filter_account_state(account_state, &orders);
        Ok((account_state, orders))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_filter_account_state() {
        let orders = vec![Order {
            id: 0,
            account_id: Address::zero(),
            buy_token: 0,
            sell_token: 1,
            buy_amount: 1,
            sell_amount: 1,
        }];
        let account_states = vec![
            ((Address::zero(), 0), 3.into()),
            ((Address::zero(), 1), 4.into()),
            ((Address::zero(), 2), 5.into()),
        ];
        let result = filter_account_state(account_states.into_iter(), &orders);
        assert_eq!(result.0.len(), 1);
        assert_eq!(result.read_balance(1, Address::zero()), 4);
    }
}
