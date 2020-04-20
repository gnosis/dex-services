// Ethereum events (logs) can be both created and removed. Removals happen if the chain reorganizes
// and ends up not including block that was previously thought to be part of the chain.
// However, the orderbook state (`State`) cannot remove events. To support this, we keep an ordered
// list of all events based on which the state is built.

use super::*;
use crate::{
    contracts::stablex_contract::{batch_exchange, StableXContract},
    models::{AccountState, Order},
    orderbook::StableXOrderBookReading,
};
use anyhow::{anyhow, Context as _, Result};
use ethcontract::{
    contract::{Event, EventData},
    web3::types::BlockId,
    H256, U256,
};
use futures::{compat::Compat01As03, stream::StreamExt as _};
use state::State;
use std::collections::BTreeMap;

/// The key by which events are sorted.
#[derive(Clone, Copy, Debug, Default, Eq, Ord, PartialEq, PartialOrd)]
struct Key {
    block_number: u64,
    /// Is included to differentiate events from the same block number but different blocks which
    /// can happen during reorgs.
    block_hash: H256,
    log_index: usize,
}

#[derive(Clone, Debug)]
struct Value {
    event: batch_exchange::Event,
    /// The batch id is calculated based on the timestamp of the block.
    batch_id: BatchId,
}

pub struct Orderbook<'a> {
    contract: &'a dyn StableXContract,
    events: BTreeMap<Key, Value>,
    web3: &'a crate::contracts::Web3,
    // Cache this so we don't have to query the node for every event even when they come from the
    // same block hash.
    block_timestamp_cache: (H256, u64),
    initialized: bool,
}

impl<'a> Orderbook<'a> {
    pub fn new(_contract: &'a dyn StableXContract) -> Result<Self> {
        // TODO: we probably need thread safety and then call init in it's own thread.
        // get_auction_data should error until initialized is true.
        unimplemented!();
    }

    async fn init(&mut self) -> Result<()> {
        // Create stream before past events to ensure we don't miss any.
        let mut stream = self.contract.stream_events();

        for event in self.contract.past_events().await? {
            self.handle_event_1(event).await?;
        }
        self.initialized = true;

        while let Some(event) = stream.next().await {
            self.handle_event_1(event.context("next event error")?)
                .await?;
        }

        Ok(())
    }

    async fn get_block_timestamp(&mut self, block_hash: H256) -> Result<u64> {
        if self.block_timestamp_cache.0 != block_hash {
            let block = Compat01As03::new(self.web3.eth().block(BlockId::Hash(block_hash)))
                .await
                .with_context(|| format!("failed to get block {}", block_hash))?
                .with_context(|| format!("block {} does not exist", block_hash))?;
            self.block_timestamp_cache = (block_hash, block.timestamp.low_u64());
        }
        Ok(self.block_timestamp_cache.1)
    }

    async fn handle_event_1(&mut self, event: Event<batch_exchange::Event>) -> Result<()> {
        match event {
            Event {
                data,
                meta: Some(meta),
            } => {
                let block_timestamp = self.get_block_timestamp(meta.block_hash).await?;
                let (event, removed) = match data {
                    EventData::Added(event) => (event, false),
                    EventData::Removed(event) => (event, true),
                };
                self.handle_event_2(
                    event,
                    removed,
                    meta.block_number,
                    meta.log_index,
                    meta.block_hash,
                    block_timestamp,
                );
                Ok(())
            }
            Event { meta: None, .. } => Err(anyhow!("event without metadata")),
        }
    }

    fn handle_event_2(
        &mut self,
        event: batch_exchange::Event,
        removed: bool,
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
        if removed {
            self.events.remove(&key);
        } else {
            self.events.insert(key, Value { event, batch_id });
        }
    }

    fn create_state(&self) -> Result<State> {
        self.events.iter().try_fold(
            State::default(),
            |state, (_key, Value { event, batch_id })| state.apply_event(event, *batch_id),
        )
    }
}

impl<'a> StableXOrderBookReading for Orderbook<'a> {
    fn get_auction_data(&self, index: U256) -> Result<(AccountState, Vec<Order>)> {
        let state = self.create_state()?;
        let batch_id = index.as_u32();
        let account_state = state.account_state(batch_id).collect();
        let account_state = AccountState(account_state);
        let orders = state.orders(batch_id).collect();
        Ok((account_state, orders))
    }
}
