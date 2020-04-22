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

/// The key by which events are sorted.
#[derive(Debug, Eq, Ord, PartialEq, PartialOrd)]
struct Key {
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

pub struct Orderbook {
    events: BTreeMap<Key, Value>,
}

impl Orderbook {
    pub fn new() -> Self {
        // TODO: Set up a thread that applies past events and listens for new events.
        unimplemented!();
    }

    fn handle_event_data(
        &mut self,
        event_data: EventData<batch_exchange::Event>,
        block_number: u64,
        log_index: usize,
        block_hash: H256,
        block_timestamp: u64,
    ) {
        let batch_id = block_timestamp as BatchId / 300;
        let key = Key {
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

impl StableXOrderBookReading for Orderbook {
    fn get_auction_data(&self, _index: U256) -> Result<(AccountState, Vec<Order>)> {
        // TODO: Handle future batch ids for when we want to do optimistic solving.
        let state = self.create_state()?;
        let (account_state, orders) = state.orderbook(Batch::Current)?;
        let account_state = account_state
            // TODO: change AccountState to use U256
            .map(|(key, value)| (key, value.low_u128()))
            .collect();
        let orders = orders.collect();
        Ok((AccountState(account_state), orders))
    }
}
