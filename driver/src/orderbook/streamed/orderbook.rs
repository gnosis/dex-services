// Ethereum events (logs) can be both created and removed. Removals happen if the chain reorganizes
// and ends up not including block that was previously thought to be part of the chain.
// However, the orderbook state (`State`) cannot remove events. To support this, we keep an ordered
// list of all events based on which the state is built. New events are applied to `State` as
// expected. When an event is removed we remove it from the list and replay all events to recreate
// the state without the removed event.
// This means that we need to keep all events in memory and we need to do a lot of work when an event
// is removed. In practice we can optimize by relying on the assumption that reorgs for a block
// become more unlikely over time.
// We store both the current state and an old state which is always n blocks behind the
// current. When an event is removed we remove it from the list of events and replay the events
// into the snapshot state.
//
// These assumptions do not hold:
// 1. New events are received in the order they have been emitted.
// 2. If an event is removed then all later events (according to `Key`) are going to be removed.
// These assumptions would make the implementation simpler and faster but cannot be relied upon.
// They cannot be relied upon because this is not guaranteed by the api documentation and they are
// not true in some current ethereum nodes.
//
// Geth: block_chain.go: `func (bc *BlockChain) reorg(oldBlock, newBlock *types.Block)`
// At the end of the function: `bc.rmLogsFeed.Send(RemovedLogsEvent{mergeLogs(deletedLogs, true)})`
// This used to not guarantee ordering between reorgs (see commit history) but now it does.
// Internally there are two channels, one for new logs and one for old logs. Each is handled
// separately in filter_system.go: `func (es *EventSystem) eventLoop()`. It does not look like it is
// guaranteed that removes are handled before "rebirths" (new logs).
//
// Since we cannot rely on these assumptions we have to accept that we might get new events or
// removals at any position in the list of events instead of just the end. This is handled by using a
// BTreeMap which sorts all events based on their block number and log index. Additionally we need
// the block hash because it is possible during a reorg to observe events for the same block number
// but different block hash.

// TODO: make sure that we only get events for mined blocks, not pending blocks. We need to have
// block_number and block_hash set but they would be null for pending blocks.
// https://github.com/ethereum/go-ethereum/blob/d90d1db609c8d77baa422d49bd371207c06b4711/eth/filters/api.go#L277

use super::{state::State, *};
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
use std::collections::BTreeMap;

/// The number of blocks the old state laggs behind the current block.
const OLD_STATE_AGE: u64 = 128;

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

#[derive(Debug, Default)]
struct PureOrderbook {
    /// The state which has all past events applied to it until the first event in `self.events`.
    /// This state is used as the starting point to which new events are replayed when a reorg
    /// happens.
    old_state: State,
    /// The state which has all past events applied to it including `self.events`.
    current_state: State,
    /// The events of the last `SNAPSHOT_AGE` number of blocks.
    events: BTreeMap<Key, Value>,
    /// True when we cannot successfully construct the current state based on `self.events`.
    /// This can happen temporarily during reorgs. If this is the case then `self.current_state`
    /// remains at an earlier consistent point.
    is_inconsistent: bool,
}

impl PureOrderbook {
    fn new() -> Self {
        Self {
            ..Default::default()
        }
    }

    /// An error indicates that updating the old state failed.
    fn apply_event(
        &mut self,
        event: batch_exchange::Event,
        removed: bool,
        block_number: u64,
        log_index: usize,
        block_hash: H256,
        block_timestamp: u64,
    ) -> Result<()> {
        let key = Key {
            block_number,
            block_hash,
            log_index,
        };

        if removed {
            // For less replays (increased performance) we could delete all events from this block
            // hash but let's do that only if we have measured performance to be a problem.
            if self.events.remove(&key).is_some() {
                self.replay_events();
            }
            return Ok(());
        }

        let batch_id = block_timestamp as BatchId / 300;
        let new_event_is_last_event = self.events.range(key..).next().is_none();
        if new_event_is_last_event {
            // If this event is not the last one then it's block number is not higher than
            // the previous highest block number so updating the old state would have no effect.
            self.update_old_state(block_number)
                .context("old state update failed")?;
        }
        if new_event_is_last_event && !self.is_inconsistent {
            self.is_inconsistent = self.current_state.apply_event(&event, batch_id).is_err();
        }
        self.events.insert(key, Value { event, batch_id });
        if !new_event_is_last_event || self.is_inconsistent {
            self.replay_events();
        }
        Ok(())
    }

    fn replay_events(&mut self) {
        self.current_state = self.old_state.clone();
        for (_, value) in self.events.iter() {
            if self
                .current_state
                .apply_event(&value.event, value.batch_id)
                .is_err()
            {
                self.is_inconsistent = true;
                return;
            }
        }
        self.is_inconsistent = false;
    }

    fn update_old_state(&mut self, current_block_number: u64) -> Result<(), state::Error> {
        let first_excluded_block = match current_block_number.checked_sub(OLD_STATE_AGE) {
            Some(block_number) => block_number,
            None => return Ok(()),
        };
        let first_excluded_key = Key {
            block_number: first_excluded_block,
            block_hash: H256::zero(),
            log_index: 0,
        };
        let excluded_events = self.events.split_off(&first_excluded_key);
        let included_events = std::mem::replace(&mut self.events, excluded_events);
        for (_, value) in included_events {
            self.old_state.apply_event(&value.event, value.batch_id)?;
        }
        Ok(())
    }
}

pub struct Orderbook<'a> {
    contract: &'a dyn StableXContract,
    orderbook: PureOrderbook,
    web3: &'a crate::contracts::Web3,
    last_block_timestamp: (H256, u64),
}

impl<'a> Orderbook<'a> {
    pub fn new() -> Self {
        unimplemented!()
    }

    async fn get_block_timestamp(&mut self, block_hash: H256) -> Result<u64> {
        // TODO: cache this so we don't have to query the node for every event
        if self.last_block_timestamp.0 != block_hash {
            let block = Compat01As03::new(self.web3.eth().block(BlockId::Hash(block_hash)))
                .await
                .with_context(|| format!("failed to get block {}", block_hash))?
                .with_context(|| format!("block {} does not exist", block_hash))?;
            self.last_block_timestamp = (block_hash, block.timestamp.low_u64());
        }
        Ok(self.last_block_timestamp.1)
    }

    async fn handle_event(&mut self, event: Event<batch_exchange::Event>) -> Result<()> {
        dbg!(&event);
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
                // Applying events to `current_state` can temporarily fail for example when
                // only some removals have been received invalidating later events.
                // This should never happen for `old_state` because it should not be affected
                // by reorgs. If it does happen our events must have gone out of sync with the node
                // or there is a bug in `State`.
                // TODO: What should we do about it? Start a full resync? Panic?
                self.orderbook.apply_event(
                    event,
                    removed,
                    meta.block_number,
                    meta.log_index,
                    meta.block_hash,
                    block_timestamp,
                )
            }
            Event { meta: None, .. } => Err(anyhow!("event without metadata")),
        }
    }

    pub async fn handle_events(&mut self) -> Result<()> {
        let mut stream = self.contract.stream_events();
        while let Some(event) = stream.next().await {
            self.handle_event(event.context("next event error")?)
                .await?;
        }
        Ok(())
    }
}

impl StableXOrderBookReading for Orderbook<'_> {
    fn get_auction_data(&self, index: U256) -> Result<(AccountState, Vec<Order>)> {
        let batch_id = index.as_u32();
        // TODO:
        // self.current_state.apply_pending_solution_if_needed(batch_id);
        let account_state = self
            .orderbook
            .current_state
            .account_state(batch_id)
            .unwrap()
            .collect();
        let account_state = AccountState(account_state);
        let orders = self
            .orderbook
            .current_state
            .orders(batch_id)
            .unwrap()
            .collect();
        Ok((account_state, orders))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::contracts::stablex_contract::{
        batch_exchange::{event_data::*, Event},
        StableXContractImpl,
    };
    use serde::{Deserialize, Serialize};
    use std::collections::HashMap;

    fn order(id: OrderId) -> Event {
        Event::OrderPlacement(OrderPlacement {
            owner: Address::from_low_u64_be(0),
            index: id,
            buy_token: 0,
            sell_token: 0,
            valid_from: 0,
            valid_until: 1,
            price_numerator: 1,
            price_denominator: 1,
        })
    }

    fn order_deletion(id: OrderId) -> Event {
        Event::OrderDeletion(OrderDeletion {
            owner: Address::from_low_u64_be(0),
            id,
        })
    }

    #[test]
    fn new_event_at_end() {
        let mut book = PureOrderbook::new();
        book.apply_event(order(0), false, 0, 0, H256::zero(), 300)
            .unwrap();
        assert_eq!(book.get_auction_data(0.into()).unwrap().1.len(), 1);
        book.apply_event(order_deletion(0), false, 0, 1, H256::zero(), 300)
            .unwrap();
        assert_eq!(book.get_auction_data(0.into()).unwrap().1.len(), 0);
    }

    #[test]
    fn new_event_in_middle() {
        let mut book = PureOrderbook::new();
        book.apply_event(order(0), false, 0, 0, H256::zero(), 300)
            .unwrap();
        book.apply_event(order_deletion(2), false, 0, 3, H256::zero(), 300)
            .unwrap();
        assert_eq!(book.get_auction_data(0.into()).unwrap().1.len(), 1);
        book.apply_event(order(1), false, 0, 1, H256::zero(), 300)
            .unwrap();
        assert_eq!(book.get_auction_data(0.into()).unwrap().1.len(), 2);
        book.apply_event(order(2), false, 0, 2, H256::zero(), 300)
            .unwrap();
        assert_eq!(book.get_auction_data(0.into()).unwrap().1.len(), 2);
    }

    #[test]
    fn removed_events() {
        let mut book = PureOrderbook::new();
        book.apply_event(order(0), false, 0, 0, H256::zero(), 300)
            .unwrap();
        book.apply_event(order(1), false, 0, 1, H256::zero(), 300)
            .unwrap();
        book.apply_event(order(2), false, 0, 2, H256::zero(), 300)
            .unwrap();
        assert_eq!(book.get_auction_data(0.into()).unwrap().1.len(), 3);
        book.apply_event(order(1), true, 0, 1, H256::zero(), 300)
            .unwrap();
        assert_eq!(book.get_auction_data(0.into()).unwrap().1.len(), 2);
        book.apply_event(order(2), true, 0, 2, H256::zero(), 300)
            .unwrap();
        assert_eq!(book.get_auction_data(0.into()).unwrap().1.len(), 1);
    }

    #[test]
    fn old_state() {
        let mut book = PureOrderbook::new();

        for i in 0..3 {
            book.apply_event(order(i), false, i as u64, 0, H256::zero(), 300)
                .unwrap();
        }
        assert_eq!(book.current_state.orders(0).unwrap().count(), 3);
        assert_eq!(book.old_state.orders(0).unwrap().count(), 0);
        assert_eq!(book.events.len(), 3);

        for i in 1..4 {
            book.apply_event(order(2 + i), false, 100 + i as u64, 0, H256::zero(), 300)
                .unwrap();
            assert_eq!(book.events.len(), 3);
            assert_eq!(
                book.current_state.orders(0).unwrap().count(),
                3 + i as usize
            );
            assert_eq!(book.old_state.orders(0).unwrap().count(), i as usize);
        }
        book.apply_event(order(6), false, 104, 0, H256::zero(), 300)
            .unwrap();
        assert_eq!(book.events.len(), 4);
        assert_eq!(book.current_state.orders(0).unwrap().count(), 7);
        assert_eq!(book.old_state.orders(0).unwrap().count(), 3);
    }

    #[derive(Clone, Debug, Default, Deserialize, Serialize)]
    struct OrderbookSerialization {
        account_state: Vec<((UserId, TokenId), u128)>,
        orders: Vec<Order>,
        batch_id: BatchId,
    }

    #[derive(Debug, Deserialize, Serialize)]
    pub struct EventsSerialization {
        pub event: crate::contracts::stablex_contract::batch_exchange::Event,
        pub block_timestamp: u64,
    }

    fn serialize_events(contract: &StableXContractImpl, web3: &crate::contracts::Web3) {
        let out = std::fs::File::create("events.json").unwrap();
        let mut orderbook = Orderbook {
            contract,
            orderbook: PureOrderbook::new(),
            web3,
            last_block_timestamp: (H256::zero(), 0),
        };
        let events = contract
            .past_events()
            .unwrap()
            .into_iter()
            .map(|e| (e.data, e.meta.unwrap()))
            .filter_map(|(data, meta)| match data {
                EventData::Added(event) => Some((event, meta)),
                EventData::Removed(_) => panic!("removed"),
            })
            .map(|(event, meta)| {
                let block_timestamp =
                    futures::executor::block_on(orderbook.get_block_timestamp(meta.block_hash))
                        .unwrap();
                EventsSerialization {
                    event,
                    block_timestamp,
                }
            })
            .collect::<Vec<_>>();
        serde_json::ser::to_writer(out, &events).unwrap();
    }

    fn serialize_paginated_orderbook(contract: &StableXContractImpl) {
        let out = std::fs::File::create("paginated.json").unwrap();
        let batch_id = contract.get_current_auction_index().unwrap();
        let reader = crate::orderbook::PaginatedStableXOrderBookReader::new(contract, 500);
        let book = reader.get_auction_data(batch_id.into()).unwrap();
        let mut account_state = (book.0).0.into_iter().collect::<Vec<_>>();
        account_state.sort();
        let mut orders = book.1;
        orders.sort();
        let book = OrderbookSerialization {
            account_state,
            orders,
            batch_id,
        };
        serde_json::ser::to_writer(out, &book).unwrap();
    }

    #[test]
    fn compare_with_paginated() {
        // Use files previously created by serialize_paginated_orderbook and serialize_events to
        // compare the resulting orderbooks.
        let paginated = std::fs::File::open("rinkeby_paginated.json").unwrap();
        let paginated: OrderbookSerialization = serde_json::from_reader(paginated).unwrap();
        let events = std::fs::File::open("rinkeby_events.json").unwrap();
        let events: Vec<EventsSerialization> = serde_json::from_reader(events).unwrap();
        let mut state = State::default();
        for event in events {
            state
                .apply_event(&event.event, event.block_timestamp as u32 / 300)
                .unwrap();
        }
        state.apply_pending_solution_if_needed(paginated.batch_id);
        let event_account_states = state
            .account_state(paginated.batch_id)
            .unwrap()
            .filter(|(_key, value)| *value > 0)
            .collect::<HashMap<_, _>>();
        let paginated_account_states = paginated
            .account_state
            .into_iter()
            .filter(|(_key, value)| *value > 0)
            .collect::<HashMap<_, _>>();
        let mut event_orders = state
            .orders(paginated.batch_id)
            .unwrap()
            .collect::<Vec<_>>();
        event_orders.sort();

        // The event based account states can contain balances that don't exist in paginated because
        // it does not include balances for tokens the user does not have an order for.
        for (key, value) in paginated_account_states.iter() {
            assert_eq!(event_account_states.get(key), Some(value));
        }
        assert_eq!(paginated.orders, event_orders);
    }

    #[test]
    #[ignore]
    fn vk() {
        let (_, _guard) = crate::logging::init("driver=debug");
        let prometheus_registry = std::sync::Arc::new(prometheus::Registry::new());
        let http_metrics = crate::metrics::HttpMetrics::new(&prometheus_registry).unwrap();
        let http_factory =
            crate::http::HttpFactory::new(std::time::Duration::from_secs(10), http_metrics);
        // TODO: had to increase timeout because getting all past logs is slow
        let web3 = crate::contracts::web3_provider(
            &http_factory,
            "https://node.rinkeby.gnosisdev.com",
            std::time::Duration::from_secs(30),
        )
        .unwrap();
        let gas_station = crate::gas_station::GnosisSafeGasStation::new(
            &http_factory,
            crate::gas_station::DEFAULT_URI,
        )
        .unwrap();
        let secret_key = ethcontract::secret::PrivateKey::from_hex_str(
            "0000000000000000000000000000000000000000000000000000000000000000",
        )
        .unwrap();
        let contract = crate::contracts::stablex_contract::StableXContractImpl::new(
            &web3,
            secret_key,
            5777,
            &gas_station,
        )
        .unwrap();
    }
}
