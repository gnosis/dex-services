use super::{
    state::{Batch, State},
    BatchId, TokenId, UserId,
};
use crate::{
    contracts::stablex_contract::batch_exchange,
    models::{AccountState, Order},
    orderbook::StableXOrderBookReading,
};
use anyhow::{Context, Result};
use ethcontract::{contract::EventData, H256, U256};
use log::info;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::convert::TryFrom;
use std::fs::File;
use std::io::{Read, Write};
use std::path::Path;
use std::{env, fs};

// Ethereum events (logs) can be both created and removed. Removals happen if the chain reorganizes
// and ends up not including block that was previously thought to be part of the chain.
// However, the orderbook state (`State`) cannot remove events. To support this, we keep an ordered
// list of all events based on which the state is built.

#[derive(Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
struct EventSortKey {
    block_number: u64,
    /// Is included to differentiate events from the same block number but different blocks which
    /// can happen during reorgs.
    block_hash: H256,
    log_index: usize,
}

#[derive(Debug, Deserialize, Eq, PartialEq, Serialize)]
struct Value {
    event: batch_exchange::Event,
    /// The batch id is calculated based on the timestamp of the block.
    batch_id: BatchId,
}

#[derive(Debug, Default, Deserialize, Serialize)]
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

    pub fn delete_events_starting_at_block(&mut self, block_number: u64) {
        self.events.split_off(&EventSortKey {
            block_number,
            block_hash: H256::zero(),
            log_index: 0,
        });
    }

    pub fn write_to_file(&self, path: impl AsRef<Path>) -> Result<()> {
        // Write to tmp file until complete and then rename.
        let mut temp_path = env::temp_dir();
        temp_path.push("temp_orderbook.ron");

        // Create temp file to be written completely before rename
        let mut temp_file = File::create(&temp_path)
            .with_context(|| format!("couldn't create {}", temp_path.display()))?;
        let file_content = ron::ser::to_string_pretty(self, ron::ser::PrettyConfig::default())?;
        temp_file.write_all(file_content.as_bytes())?;

        // Rename the temp file to the originally specified path.
        fs::rename(temp_path, path)?;
        Ok(())
    }

    fn create_state(&self) -> Result<State> {
        self.events
            .iter()
            .try_fold(State::default(), |state, (_key, value)| {
                state.apply_event(&value.event, value.batch_id)
            })
    }
}

impl TryFrom<&[u8]> for Orderbook {
    type Error = anyhow::Error;

    fn try_from(bytes: &[u8]) -> Result<Self> {
        ron::de::from_bytes(bytes).context("Failed to load Orderbook")
    }
}

impl TryFrom<File> for Orderbook {
    type Error = anyhow::Error;

    fn try_from(mut file: File) -> Result<Self> {
        let mut contents = String::new();
        let bytes_read = file
            .read_to_string(&mut contents)
            .with_context(|| format!("Failed to read file: {:?}", file))?;
        info!(
            "Successfully loaded {} bytes from Orderbook file",
            bytes_read
        );
        Orderbook::try_from(contents.as_bytes())
    }
}

impl TryFrom<&Path> for Orderbook {
    type Error = anyhow::Error;

    fn try_from(path: &Path) -> Result<Self> {
        let file = File::open(path).with_context(|| format!("couldn't open {}", path.display()))?;
        Orderbook::try_from(file)
    }
}

fn filter_auction_data(
    account_states: impl IntoIterator<Item = ((UserId, TokenId), U256)>,
    orders: impl IntoIterator<Item = Order>,
) -> (AccountState, Vec<Order>) {
    let orders = orders
        .into_iter()
        .filter(|order| order.sell_amount > 0)
        .collect::<Vec<_>>();
    let account_states = account_states
        .into_iter()
        .filter(|((user, token), _)| {
            orders
                .iter()
                .any(|order| order.account_id == *user && order.sell_token == *token)
        })
        // TODO: change AccountState to use U256
        .map(|(key, value)| (key, value.low_u128()))
        .collect();
    (AccountState(account_states), orders)
}

impl StableXOrderBookReading for Orderbook {
    fn get_auction_data(&self, batch_id_to_solve: U256) -> Result<(AccountState, Vec<Order>)> {
        // TODO: Handle future batch ids for when we want to do optimistic solving.
        let state = self.create_state()?;
        // `orderbook_for_batch` takes the index of the auction that is currently collecting orders and returns
        // the orderbook for the batch index that is currently being solved. `get_auction_data` passed in the
        // index for the auction that orders should be valid for (the one currently being solved). Thus we need
        // to increment it.
        let (account_state, orders) =
            state.orderbook_for_batch(Batch::Future(batch_id_to_solve.low_u32() + 1))?;
        let (account_state, orders) = filter_auction_data(account_state, orders);
        Ok((account_state, orders))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::contracts::stablex_contract::batch_exchange::event_data::*;
    use crate::contracts::stablex_contract::batch_exchange::Event;
    use ethcontract::Address;

    #[test]
    fn test_filter_account_state() {
        let orders = vec![
            Order {
                id: 0,
                account_id: Address::zero(),
                buy_token: 0,
                sell_token: 1,
                buy_amount: 1,
                sell_amount: 1,
            },
            Order {
                id: 0,
                account_id: Address::repeat_byte(1),
                buy_token: 0,
                sell_token: 2,
                buy_amount: 0,
                sell_amount: 0,
            },
        ];
        let account_states = vec![
            ((Address::zero(), 0), 3.into()),
            ((Address::zero(), 1), 4.into()),
            ((Address::zero(), 2), 5.into()),
        ];

        let (account_state, orders) = filter_auction_data(account_states, orders);
        assert_eq!(account_state.0.len(), 1);
        assert_eq!(account_state.read_balance(1, Address::zero()), 4);
        assert_eq!(orders.len(), 1);
        assert_eq!(orders[0].account_id, Address::zero());
    }

    #[test]
    fn test_write_read_recover_full_cycle() {
        let event_key = EventSortKey {
            block_number: 0,
            block_hash: H256::zero(),
            log_index: 1,
        };
        let event = Event::Deposit(Deposit {
            user: Address::from_low_u64_be(1),
            token: Address::from_low_u64_be(2),
            amount: 1.into(),
            batch_id: 2,
        });
        let value = Value { event, batch_id: 0 };

        let mut events = BTreeMap::new();
        events.insert(event_key, value);
        let initial_orderbook = Orderbook { events };

        let test_path = Path::new("/tmp/my_test_orderbook.ron");
        initial_orderbook.write_to_file(test_path).unwrap();

        let recovered_orderbook = Orderbook::try_from(test_path).unwrap();
        assert_eq!(initial_orderbook.events, recovered_orderbook.events);

        // Cleanup the file created here.
        assert!(fs::remove_file(test_path).is_ok());
    }
}
