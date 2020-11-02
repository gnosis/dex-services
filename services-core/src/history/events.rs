use crate::{
    models::{AccountState, BatchId, Order},
    orderbook::streamed::State,
    serialization::Version,
};
use anyhow::{Context, Result};
use contracts::batch_exchange;
use ethcontract::{BlockNumber, H256};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    convert::TryFrom,
    fs::{self, File},
    io::{BufReader, BufWriter, Read, Write},
    ops::Bound,
    path::Path,
};
use typenum::U1;

// Ethereum events (logs) can be both created and removed. Removals happen if the chain reorganizes
// and ends up not including block that was previously thought to be part of the chain.
// However, the orderbook state (`State`) cannot remove events. To support this, we keep an ordered
// list of all events based on which the state is built.

#[derive(Clone, Debug, Default, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
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
pub struct EventRegistry {
    version: Version<U1>,
    events: BTreeMap<EventSortKey, Value>,
}

impl EventRegistry {
    pub fn read(reader: impl Read) -> Result<Self> {
        Ok(bincode::deserialize_from(reader)?)
    }

    pub fn handle_event_data(
        &mut self,
        event: batch_exchange::Event,
        block_number: u64,
        log_index: usize,
        block_hash: H256,
        block_timestamp: u64,
    ) {
        let batch_id = BatchId::from_timestamp(block_timestamp);
        let key = EventSortKey {
            block_number,
            block_hash,
            log_index,
        };
        self.events.insert(key, Value { event, batch_id });
    }

    pub fn delete_events_starting_at_block(&mut self, block_number: u64) {
        self.events.split_off(&EventSortKey {
            block_number,
            block_hash: H256::zero(),
            log_index: 0,
        });
    }

    /// Serializes an `EventRegistry` into its bincode representation.
    #[cfg(test)]
    pub fn to_bytes(&self) -> Result<Vec<u8>> {
        Ok(bincode::serialize(self)?)
    }

    pub fn write_to(&self, writer: impl Write) -> Result<()> {
        bincode::serialize_into(writer, self)?;
        Ok(())
    }

    pub fn write_to_file(&self, path: impl AsRef<Path>) -> Result<()> {
        // Write to tmp file until complete and then rename.
        let temp_path = path.as_ref().with_extension("temp");
        {
            // Create temp file to be written completely before rename
            let temp_file = File::create(&temp_path)
                .with_context(|| format!("couldn't create {}", temp_path.display()))?;

            let mut buffered_writer = BufWriter::new(temp_file);
            self.write_to(&mut buffered_writer)?;
            buffered_writer.flush()?;
        }
        // Rename the temp file to the originally specified path.
        fs::rename(temp_path, path)?;
        Ok(())
    }

    pub fn last_handled_block(&self) -> Option<u64> {
        Some(self.events.iter().next_back()?.0.block_number)
    }

    /// Returns an iterator over all owned events and their corresponding batch
    /// IDs.
    pub fn into_events(self) -> impl Iterator<Item = (batch_exchange::Event, BatchId)> {
        self.events
            .into_iter()
            .map(|(_, Value { event, batch_id })| (event, batch_id))
    }

    /// Returns an iterator of all events and their corresponding batch IDs.
    /// This is a borrowed version of the `into_events` method.
    pub fn events(
        &self,
    ) -> impl DoubleEndedIterator<Item = (&'_ batch_exchange::Event, BatchId)> + '_ {
        self.events
            .values()
            .map(|Value { event, batch_id }| (event, *batch_id))
    }

    /// Returns an iterator containing all events up to and including the
    /// specified batch ID.
    pub fn events_until_batch(
        &self,
        batch_id: impl Into<BatchId>,
    ) -> impl Iterator<Item = (&'_ batch_exchange::Event, BatchId)> + '_ {
        let batch_id = batch_id.into();
        self.events()
            .take_while(move |(_, event_batch_id)| *event_batch_id <= batch_id)
    }

    /// Returns an iterator containing all events that occured in the specified
    /// batch ID.
    pub fn events_for_batch(
        &self,
        batch_id: impl Into<BatchId>,
    ) -> impl Iterator<Item = &'_ batch_exchange::Event> + '_ {
        let batch_id = batch_id.into();
        self.events()
            .skip_while(move |(_, event_batch_id)| *event_batch_id < batch_id)
            .take_while(move |(_, event_batch_id)| *event_batch_id == batch_id)
            .map(|(event, _)| event)
    }

    /// Create a new streamed orderbook auction state with events from batches
    /// up to and including the specified batch ID.
    pub fn auction_state_for_batch(
        &self,
        batch_id: impl Into<BatchId>,
    ) -> Result<(AccountState, Vec<Order>)> {
        let batch_id = batch_id.into();
        auction_state_for_batch_from_events(batch_id, self.events_until_batch(batch_id))
    }

    /// Create a new orderbook auction state with events up to and including
    /// block number for solving the specified batch.
    pub fn auction_state_for_batch_at_block(
        &self,
        batch_id: impl Into<BatchId>,
        block: impl Into<BlockNumber>,
    ) -> Result<(AccountState, Vec<Order>)> {
        let block_number = match block.into() {
            BlockNumber::Earliest => 0,
            BlockNumber::Latest | BlockNumber::Pending => u64::MAX,
            BlockNumber::Number(block_number) => block_number.as_u64(),
        };
        auction_state_for_batch_from_events(
            batch_id,
            self.events
                .range(bounds_until_end_of_block(block_number))
                .map(|(_, Value { event, batch_id })| (event, *batch_id)),
        )
    }
}

fn auction_state_for_batch_from_events<'a>(
    batch_id: impl Into<BatchId>,
    events: impl Iterator<Item = (&'a batch_exchange::Event, BatchId)>,
) -> Result<(AccountState, Vec<Order>)> {
    let batch_id = batch_id.into();
    let state = State::from_events(events.map(|(event, batch_id)| (event, batch_id.into())))?;

    // In order to solve batch t we need the orderbook at the beginning of
    // batch t+1's collection process
    state.canonicalized_auction_state_at_beginning_of_batch(batch_id.next().into())
}

impl TryFrom<File> for EventRegistry {
    type Error = anyhow::Error;

    fn try_from(mut file: File) -> Result<Self> {
        let buffered_reader = BufReader::new(&mut file);
        let events = EventRegistry::read(buffered_reader)
            .with_context(|| format!("Failed to read file: {:?}", file))?;

        log::info!(
            "Successfully loaded {} events in {} bytes from event registry file",
            events.events.len(),
            file.metadata()?.len(),
        );

        Ok(events)
    }
}

impl TryFrom<&Path> for EventRegistry {
    type Error = anyhow::Error;

    fn try_from(path: &Path) -> Result<Self> {
        let file = File::open(path).with_context(|| format!("couldn't open {}", path.display()))?;
        EventRegistry::try_from(file)
    }
}

fn bounds_until_end_of_block(block_number: u64) -> (Bound<EventSortKey>, Bound<EventSortKey>) {
    (
        Bound::Unbounded,
        Bound::Excluded(EventSortKey {
            block_number: block_number.saturating_add(1),
            ..Default::default()
        }),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use contracts::batch_exchange::{event_data::*, Event};
    use ethcontract::{Address, U256};

    #[test]
    fn test_serialize_deserialize_events() {
        let event_list: Vec<Event> = vec![
            Event::Deposit(Deposit::default()),
            Event::WithdrawRequest(WithdrawRequest::default()),
            Event::Withdraw(Withdraw::default()),
            Event::TokenListing(TokenListing::default()),
            Event::OrderPlacement(OrderPlacement::default()),
            Event::OrderCancellation(OrderCancellation::default()),
            Event::OrderDeletion(OrderDeletion::default()),
            Event::Trade(Trade::default()),
            Event::TradeReversion(TradeReversion::default()),
            Event::SolutionSubmission(SolutionSubmission::default()),
        ];

        let events: BTreeMap<EventSortKey, Value> = event_list
            .iter()
            .enumerate()
            .map(|(i, event)| {
                (
                    EventSortKey {
                        block_number: i as u64,
                        block_hash: H256::zero(),
                        log_index: 1,
                    },
                    Value {
                        event: event.clone(),
                        batch_id: 0.into(),
                    },
                )
            })
            .collect();
        let events = EventRegistry {
            version: Default::default(),
            events,
        };
        let serialized_events = bincode::serialize(&events).expect("Failed to serialize events");
        let deserialized_events =
            EventRegistry::read(&serialized_events[..]).expect("Failed to deserialize events");
        assert_eq!(events.events, deserialized_events.events);
    }

    #[test]
    #[ignore]
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
        let value = Value {
            event,
            batch_id: 0.into(),
        };

        let mut events = BTreeMap::new();
        events.insert(event_key, value);
        let initial_events = EventRegistry {
            version: Default::default(),
            events,
        };

        let test_path = Path::new("/tmp/my_test_events.ron");
        initial_events.write_to_file(test_path).unwrap();

        let recovered_events = EventRegistry::try_from(test_path).unwrap();
        assert_eq!(initial_events.events, recovered_events.events);

        // Cleanup the file created here.
        assert!(fs::remove_file(test_path).is_ok());
    }

    #[test]
    fn delete_events_starting_at_block() {
        let mut events = EventRegistry::default();
        let token_listing_0 = Event::TokenListing(TokenListing {
            token: Address::from_low_u64_be(0),
            id: 0,
        });
        events.handle_event_data(token_listing_0, 0, 0, H256::zero(), 0);
        let token_listing_1 = Event::TokenListing(TokenListing {
            token: Address::from_low_u64_be(1),
            id: 1,
        });
        events.handle_event_data(token_listing_1, 0, 1, H256::zero(), 0);
        let order_placement = Event::OrderPlacement(OrderPlacement {
            owner: Address::from_low_u64_be(2),
            index: 0,
            buy_token: 1,
            sell_token: 0,
            valid_from: 0,
            valid_until: 10,
            price_numerator: 10,
            price_denominator: 10,
        });
        events.handle_event_data(order_placement, 0, 2, H256::zero(), 0);

        let deposit_0 = Event::Deposit(Deposit {
            user: Address::from_low_u64_be(2),
            token: Address::from_low_u64_be(0),
            amount: 1.into(),
            batch_id: 0,
        });
        events.handle_event_data(deposit_0, 0, 3, H256::zero(), 0);
        let deposit_1 = Event::Deposit(Deposit {
            user: Address::from_low_u64_be(2),
            token: Address::from_low_u64_be(0),
            amount: 1.into(),
            batch_id: 0,
        });
        events.handle_event_data(deposit_1, 1, 0, H256::zero(), 0);
        let deposit_2 = Event::Deposit(Deposit {
            user: Address::from_low_u64_be(2),
            token: Address::from_low_u64_be(0),
            amount: 1.into(),
            batch_id: 0,
        });
        events.handle_event_data(deposit_2, 2, 0, H256::zero(), 0);

        let auction_data = events.auction_state_for_batch(2).unwrap();
        assert_eq!(
            auction_data.0.read_balance(0, Address::from_low_u64_be(2)),
            U256::from(3)
        );
        events.delete_events_starting_at_block(1);
        let auction_data = events.auction_state_for_batch(1).unwrap();
        assert_eq!(
            auction_data.0.read_balance(0, Address::from_low_u64_be(2)),
            U256::from(1)
        );
    }

    #[test]
    fn events_get_sorted() {
        // We add a token, deposit that token, request to withdraw that token, withdraw it but give
        // the events to the event registry in a shuffled order.
        let token_listing_0 = Event::TokenListing(TokenListing {
            token: Address::from_low_u64_be(0),
            id: 0,
        });
        let token_listing_1 = Event::TokenListing(TokenListing {
            token: Address::from_low_u64_be(1),
            id: 1,
        });
        let order_placement = Event::OrderPlacement(OrderPlacement {
            owner: Address::from_low_u64_be(2),
            index: 0,
            buy_token: 1,
            sell_token: 0,
            valid_from: 0,
            valid_until: 10,
            price_numerator: 10,
            price_denominator: 10,
        });
        let deposit = Event::Deposit(Deposit {
            user: Address::from_low_u64_be(2),
            token: Address::from_low_u64_be(0),
            amount: 10.into(),
            batch_id: 0,
        });
        let withdraw_request = Event::WithdrawRequest(WithdrawRequest {
            user: Address::from_low_u64_be(2),
            token: Address::from_low_u64_be(0),
            amount: 5.into(),
            batch_id: 0,
        });
        let withdraw = Event::Withdraw(Withdraw {
            user: Address::from_low_u64_be(2),
            token: Address::from_low_u64_be(0),
            amount: 3.into(),
        });

        let mut events = EventRegistry::default();
        events.handle_event_data(token_listing_1, 0, 1, H256::zero(), 0);
        events.handle_event_data(order_placement, 0, 2, H256::zero(), 0);
        events.handle_event_data(withdraw, 2, 0, H256::zero(), BatchId(1).as_timestamp());
        events.handle_event_data(deposit, 0, 3, H256::zero(), 0);
        events.handle_event_data(withdraw_request, 1, 0, H256::zero(), 0);
        events.handle_event_data(token_listing_0, 0, 0, H256::zero(), 0);

        let auction_data = events.auction_state_for_batch(2).unwrap();
        assert_eq!(
            auction_data.0.read_balance(0, Address::from_low_u64_be(2)),
            U256::from(7)
        );
    }

    #[test]
    fn historic_auction_states() {
        let token_listing_0 = Event::TokenListing(TokenListing {
            token: Address::from_low_u64_be(0),
            id: 0,
        });
        let token_listing_1 = Event::TokenListing(TokenListing {
            token: Address::from_low_u64_be(1),
            id: 1,
        });
        let order_placement = Event::OrderPlacement(OrderPlacement {
            owner: Address::from_low_u64_be(2),
            index: 0,
            buy_token: 1,
            sell_token: 0,
            valid_from: 0,
            valid_until: 10,
            price_numerator: 100,
            price_denominator: 100,
        });
        let deposit_0 = Event::Deposit(Deposit {
            user: Address::from_low_u64_be(2),
            token: Address::from_low_u64_be(0),
            amount: 42.into(),
            batch_id: 0,
        });
        let deposit_1 = Event::Deposit(Deposit {
            user: Address::from_low_u64_be(2),
            token: Address::from_low_u64_be(0),
            amount: 1337.into(),
            batch_id: 1,
        });

        let mut events = EventRegistry::default();
        events.handle_event_data(token_listing_0, 0, 0, H256::zero(), 0);
        events.handle_event_data(token_listing_1, 0, 1, H256::zero(), 0);
        events.handle_event_data(order_placement, 0, 2, H256::zero(), 0);
        events.handle_event_data(deposit_0, 0, 3, H256::zero(), 0);
        events.handle_event_data(deposit_1, 1, 0, H256::zero(), BatchId(2).as_timestamp());

        let auction_data = events.auction_state_for_batch(0).unwrap();
        assert_eq!(
            auction_data.0.read_balance(0, Address::from_low_u64_be(2)),
            U256::from(42)
        );
    }

    #[test]
    fn auction_state_at_block_for_batch() {
        let token_listing_0 = Event::TokenListing(TokenListing {
            token: Address::from_low_u64_be(0),
            id: 0,
        });
        let token_listing_1 = Event::TokenListing(TokenListing {
            token: Address::from_low_u64_be(1),
            id: 1,
        });
        let order_placement = Event::OrderPlacement(OrderPlacement {
            owner: Address::from_low_u64_be(2),
            index: 0,
            buy_token: 1,
            sell_token: 0,
            valid_from: 0,
            valid_until: 10,
            price_numerator: 100,
            price_denominator: 100,
        });
        let deposit_0 = Event::Deposit(Deposit {
            user: Address::from_low_u64_be(2),
            token: Address::from_low_u64_be(0),
            amount: 42.into(),
            batch_id: 0,
        });
        let deposit_1 = Event::Deposit(Deposit {
            user: Address::from_low_u64_be(2),
            token: Address::from_low_u64_be(0),
            amount: 1337.into(),
            batch_id: 0,
        });
        let deposit_2 = Event::Deposit(Deposit {
            user: Address::from_low_u64_be(2),
            token: Address::from_low_u64_be(0),
            amount: 1337.into(),
            batch_id: 1,
        });

        let mut events = EventRegistry::default();
        events.handle_event_data(token_listing_0, 0, 0, H256::zero(), 0);
        events.handle_event_data(token_listing_1, 0, 1, H256::zero(), 0);
        events.handle_event_data(order_placement, 0, 2, H256::zero(), 0);
        events.handle_event_data(deposit_0, 0, 3, H256::zero(), 0);
        events.handle_event_data(deposit_1, 1, 0, H256::zero(), 0);
        events.handle_event_data(deposit_2, 2, 0, H256::zero(), BatchId(1).as_timestamp());

        let auction_data = events.auction_state_for_batch_at_block(1, 0).unwrap();
        assert_eq!(
            auction_data.0.read_balance(0, Address::from_low_u64_be(2)),
            U256::from(42)
        );

        let auction_data = events.auction_state_for_batch_at_block(1, 1).unwrap();
        assert_eq!(
            auction_data.0.read_balance(0, Address::from_low_u64_be(2)),
            U256::from(42 + 1337)
        );
    }

    #[test]
    fn errors_when_requesting_past_batch_in_future_block() {
        let token_listing = Event::TokenListing(TokenListing {
            token: Address::from_low_u64_be(0),
            id: 0,
        });

        let mut events = EventRegistry::default();
        events.handle_event_data(
            token_listing,
            1,
            0,
            H256::zero(),
            BatchId(1337).as_timestamp(),
        );

        assert!(events
            .auction_state_for_batch_at_block(BatchId(42), 1)
            .is_err());
    }

    #[test]
    fn filters_events_by_batch_range() {
        fn token_listing(token: u16) -> Event {
            Event::TokenListing(TokenListing {
                token: Address::repeat_byte(token as _),
                id: token,
            })
        }

        let mut events = EventRegistry::default();
        events.handle_event_data(
            token_listing(0),
            0,
            0,
            H256::zero(),
            BatchId(0).as_timestamp(),
        );
        events.handle_event_data(
            token_listing(1),
            0,
            1,
            H256::zero(),
            BatchId(0).as_timestamp(),
        );
        events.handle_event_data(
            token_listing(2),
            1,
            0,
            H256::zero(),
            BatchId(1).as_timestamp(),
        );
        events.handle_event_data(
            token_listing(3),
            2,
            0,
            H256::zero(),
            BatchId(1).as_timestamp(),
        );
        events.handle_event_data(
            token_listing(4),
            3,
            0,
            H256::zero(),
            BatchId(2).as_timestamp(),
        );
        events.handle_event_data(
            token_listing(5),
            4,
            0,
            H256::zero(),
            BatchId(5).as_timestamp(),
        );

        assert_eq!(
            events.events_until_batch(1).collect::<Vec<_>>(),
            vec![
                (&token_listing(0), BatchId(0)),
                (&token_listing(1), BatchId(0)),
                (&token_listing(2), BatchId(1)),
                (&token_listing(3), BatchId(1)),
            ]
        );
        assert_eq!(
            events.events_for_batch(2).collect::<Vec<_>>(),
            vec![&token_listing(4)],
        );
        assert_eq!(events.events_for_batch(4).next(), None);
        assert_eq!(events.events_for_batch(42).next(), None);
    }
}
