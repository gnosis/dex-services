use super::util;
use super::EventHandler;
use super::*;

use dfusion_core::database::DbInterface;
use dfusion_core::models::util::{PopFromLogData, ToValue};
use dfusion_core::models::Deserializable;
use dfusion_core::models::Solution;

use failure::Error;

use graph::components::ethereum::LightEthereumBlock;
use graph::components::store::EntityOperation;
use graph::data::store::Entity;

use web3::types::{Log, Transaction};
use web3::types::{H256, U256};

use slog::{debug, info, Logger};

use std::fmt;
use std::sync::Arc;

#[derive(Clone)]
pub struct AuctionSettlementHandler {
    store: Arc<dyn DbInterface>,
}

impl AuctionSettlementHandler {
    pub fn new(store: Arc<dyn DbInterface>) -> Self {
        AuctionSettlementHandler { store }
    }
}

impl Debug for AuctionSettlementHandler {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "AuctionSettlementHandler")
    }
}

impl EventHandler for AuctionSettlementHandler {
    fn process_event(
        &self,
        logger: Logger,
        _block: Arc<LightEthereumBlock>,
        _transaction: Arc<Transaction>,
        log: Arc<Log>,
    ) -> Result<Vec<EntityOperation>, Error> {
        let mut event_data: Vec<u8> = log.data.0.clone();
        info!(
            logger,
            "Parsing Auction Settlement event from {} bytes: {:?}",
            event_data.len(),
            event_data
        );
        let auction_id = U256::pop_from_log_data(&mut event_data);
        let state_index = U256::pop_from_log_data(&mut event_data).saturating_sub(U256::one());
        let new_state_hash = H256::pop_from_log_data(&mut event_data);

        // Meta-data coming from event attributes of type bytes (cf. Ethereum RLP Encoding)
        let _bytes_init = u16::pop_from_log_data(&mut event_data);
        let _byte_size = u16::pop_from_log_data(&mut event_data);

        // Remaining information is the Auction Results
        let encoded_solution = event_data;

        // Fetch relevant information for transition (accounts, orders, parsed solution)
        let mut account_state = self
            .store
            .get_balances_for_state_index(&state_index)
            .map_err(|e| failure::err_msg(format!("{}", e)))?;

        let mut orders = self
            .store
            .get_orders_of_slot(&auction_id)
            .map_err(|e| failure::err_msg(format!("{}", e)))?;

        let standing_orders = self
            .store
            .get_standing_orders_of_slot(&auction_id)
            .map_err(|e| failure::err_msg(format!("{}", e)))?;

        orders.extend(
            standing_orders
                .iter()
                .filter(|standing_order| standing_order.num_orders() > 0)
                .flat_map(|standing_order| standing_order.get_orders().clone()),
        );

        info!(
            logger,
            "Parsing packed Auction Results from {} bytes: {:?}",
            encoded_solution.len(),
            encoded_solution
        );
        let auction_results = Solution::from_bytes(encoded_solution);
        debug!(logger, "Parsed Auction Results: {:?}", auction_results);

        info!(
            logger,
            "Found {} valid Orders for this auction",
            orders.len()
        );
        account_state.apply_auction(&orders, auction_results);

        let mut entity: Entity = account_state.into();

        // Set the state root according to event information
        entity.set("id", new_state_hash.to_value());
        Ok(vec![EntityOperation::Set {
            key: util::entity_key("AccountState", &entity),
            data: entity,
        }])
    }
}

#[cfg(test)]
pub mod unit_test {
    use super::*;
    use dfusion_core::database::tests::DbInterfaceMock;
    use dfusion_core::database::{DatabaseError, ErrorKind};
    use dfusion_core::models::{AccountState, BatchInformation, Order, StandingOrder};
    use web3::types::{Bytes, H160, H256, U256};

    #[test]
    fn test_from_log() {
        let store = Arc::new(DbInterfaceMock::new());

        // Add previous account state and pending deposits into Store
        let existing_state = AccountState::new(H256::zero(), U256::from(0), vec![2, 0, 0, 0], 2);
        store
            .get_balances_for_state_index
            .given(U256::zero())
            .will_return(Ok(existing_state));

        let order = Order {
            batch_information: Some(BatchInformation {
                slot: U256::zero(),
                slot_index: 0,
            }),
            account_id: H160::from_low_u64_be(0),
            buy_token: 1,
            sell_token: 0,
            buy_amount: 1,
            sell_amount: 1,
        };

        store
            .get_orders_of_slot
            .given(U256::zero())
            .will_return(Ok(vec![order]));

        store
            .get_standing_orders_of_slot
            .given(U256::zero())
            .will_return(Ok(StandingOrder::empty_array()));

        // Process event
        let handler = AuctionSettlementHandler::new(store);
        let log = create_auction_settlement_event(0, 1, H256::from_low_u64_be(1));

        let result = handler.process_event(
            util::test::logger(),
            Arc::new(util::test::fake_block()),
            Arc::new(util::test::fake_tx()),
            log,
        );

        assert!(result.is_ok());
        let expected_new_state =
            AccountState::new(H256::from_low_u64_be(1), U256::from(1), vec![0, 1, 0, 0], 2);
        match result.unwrap().pop().unwrap() {
            EntityOperation::Set { data, .. } => {
                assert_eq!(AccountState::from(data), expected_new_state)
            }
            _ => unreachable!(),
        }
    }

    #[test]
    fn test_auction_settlement_fails_if_state_does_not_exist() {
        let store = Arc::new(DbInterfaceMock::new());

        // No data in store
        store
            .get_balances_for_state_index
            .given(U256::zero())
            .will_return(Err(DatabaseError::new(
                ErrorKind::StateError,
                "No State found",
            )));

        let handler = AuctionSettlementHandler::new(store);
        let log = create_auction_settlement_event(0, 1, H256::from_low_u64_be(1));

        let result = handler.process_event(
            util::test::logger(),
            Arc::new(util::test::fake_block()),
            Arc::new(util::test::fake_tx()),
            log,
        );
        assert!(result.is_err());
    }

    #[test]
    fn test_auction_settlement_fails_if_orders_dont_exist() {
        let store = Arc::new(DbInterfaceMock::new());

        let existing_state = AccountState::new(H256::zero(), U256::from(0), vec![2, 0, 0, 0], 2);
        store
            .get_balances_for_state_index
            .given(U256::zero())
            .will_return(Ok(existing_state));

        store
            .get_orders_of_slot
            .given(U256::zero())
            .will_return(Ok(vec![]));

        let handler = AuctionSettlementHandler::new(store);
        let log = create_auction_settlement_event(0, 1, H256::from_low_u64_be(1));

        let result = handler.process_event(
            util::test::logger(),
            Arc::new(util::test::fake_block()),
            Arc::new(util::test::fake_tx()),
            log,
        );
        assert!(result.is_err());
    }

    #[test]
    fn test_auction_settlement_fails_if_standing_orders_dont_exist() {
        let store = Arc::new(DbInterfaceMock::new());

        let existing_state = AccountState::new(H256::zero(), U256::from(0), vec![2, 0, 0, 0], 2);
        store
            .get_balances_for_state_index
            .given(U256::zero())
            .will_return(Ok(existing_state));

        // No orders in store
        store
            .get_orders_of_slot
            .given(U256::zero())
            .will_return(Err(DatabaseError::new(
                ErrorKind::StateError,
                "No Orders found",
            )));

        let handler = AuctionSettlementHandler::new(store);
        let log = create_auction_settlement_event(0, 1, H256::from_low_u64_be(1));

        let result = handler.process_event(
            util::test::logger(),
            Arc::new(util::test::fake_block()),
            Arc::new(util::test::fake_tx()),
            log,
        );
        assert!(result.is_err());
    }
    fn create_auction_settlement_event(
        auction_id: u8,
        new_state_index: u8,
        new_state_root: H256,
    ) -> Arc<Log> {
        const NUM_TOKENS: u16 = 30;
        let mut bytes: Vec<Vec<u8>> = vec![
            /* auction_id */
            vec![
                0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                0, 0, 0, auction_id,
            ],
            /* new_state_index */
            vec![
                0,
                0,
                0,
                0,
                0,
                0,
                0,
                0,
                0,
                0,
                0,
                0,
                0,
                0,
                0,
                0,
                0,
                0,
                0,
                0,
                0,
                0,
                0,
                0,
                0,
                0,
                0,
                0,
                0,
                0,
                0,
                new_state_index,
            ],
            /* new_state_hash */ new_state_root[..].to_vec(),
            /* byte_init */ vec![0; 32],
            /* byte_length */ vec![0; 32],
        ];

        for _i in 0..NUM_TOKENS {
            bytes.push(vec![0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1]);
        }
        bytes.push(
            /* executed_buy_amount_1 */ vec![0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1],
        );
        bytes.push(
            /* executed_sell_amount_1 */ vec![0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 2],
        );

        Arc::new(Log {
            address: H160::from_low_u64_be(1),
            topics: vec![],
            data: Bytes(bytes.iter().flat_map(|i| i.iter()).cloned().collect()),
            block_hash: Some(H256::from_low_u64_be(2)),
            block_number: Some(1.into()),
            transaction_hash: Some(H256::from_low_u64_be(3)),
            transaction_index: Some(0.into()),
            log_index: Some(0.into()),
            transaction_log_index: Some(0.into()),
            log_type: None,
            removed: None,
        })
    }
}
