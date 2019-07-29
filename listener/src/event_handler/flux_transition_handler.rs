use super::*;

use dfusion_core::models::{PendingFlux, AccountState};
use dfusion_core::models::util::{PopFromLogData, ToValue};

use graph::components::store::{EntityFilter, Store};
use graph::data::store::Entity;
use std::fmt;
use web3::types::{H256, U256};

#[derive(Clone)]
pub struct FluxTransitionHandler {
    store: Arc<Store>,
}

impl FluxTransitionHandler {
    pub fn new(store: Arc<Store>) -> Self {
        FluxTransitionHandler{
            store
        }
    }
}

impl Debug for FluxTransitionHandler {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "FluxTransitionHandler")
    }
}

enum FluxTransitionType {
    Deposit = 0,
    Withdraw = 1,
}

impl From<u8> for FluxTransitionType {
    fn from(transition_type: u8) -> Self {
        match transition_type {
            0 => FluxTransitionType::Deposit,
            1 => FluxTransitionType::Withdraw,
            _ => panic!("Unknown transition type: {}", transition_type),
        }
    }
}

impl EventHandler for FluxTransitionHandler {
    fn process_event(
        &self,
        logger: Logger,
        _block: Arc<EthereumBlock>,
        _transaction: Arc<Transaction>,
        log: Arc<Log>
    ) -> Result<Vec<EntityOperation>, Error> {
        let mut data = log.data.0.clone();
        let transition_type: FluxTransitionType = u8::pop_from_log_data(&mut data).into();
        let state_index = U256::pop_from_log_data(&mut data).saturating_sub(U256::one());
        let new_state_hash = H256::pop_from_log_data(&mut data);
        let slot = U256::pop_from_log_data(&mut data);

        info!(logger, "Received Flux AccountState Transition Event");

        let account_query = util::entity_query(
            "AccountState", EntityFilter::Equal("stateIndex".to_string(), state_index.to_value())
        );
        let mut account_state = AccountState::from(self.store
            .find_one(account_query)?
            .ok_or_else(|| failure::err_msg(format!("No state record found for index {}", &state_index)))?
        );

        match transition_type {
            FluxTransitionType::Deposit => {
                let deposit_query = util::entity_query(
                    "Deposit", EntityFilter::Equal("slot".to_string(), slot.to_value())
                );
                let deposits = self.store
                    .find(deposit_query)?
                    .into_iter()
                    .map(PendingFlux::from)
                    .collect::<Vec<PendingFlux>>();
                account_state.apply_deposits(&deposits);
            },
            FluxTransitionType::Withdraw => {
                let withdraw_query = util::entity_query(
                    "Withdraw", EntityFilter::Equal("slot".to_string(), slot.to_value())
                );
                let withdraws = self.store
                    .find(withdraw_query)?
                    .into_iter()
                    .map(PendingFlux::from)
                    .collect::<Vec<PendingFlux>>();
                account_state.apply_withdraws(&withdraws);
            }
        }
        let mut entity: Entity = account_state.into();
        // We set the state root as claimed by the event
        entity.set("id", new_state_hash.to_value());
        Ok(vec![
            EntityOperation::Set {
                key: util::entity_key("AccountState", &entity),
                data: entity
            }
        ])
    }
}

#[cfg(test)]
pub mod test {
    use super::*;
    use graph_mock::MockStore;
    use web3::types::{Bytes, H256};

    #[test]
    fn test_applies_deposits_existing_state() {
        let schema = util::test::fake_schema();
        let store = Arc::new(MockStore::new(vec![(schema.id.clone(), schema)]));
        let handler = FluxTransitionHandler::new(store.clone());

        // Add previous account state and pending deposits into Store
        let existing_state = AccountState::new(H256::zero(), U256::zero(), vec![0, 0, 0, 0], 1);
        let entity = existing_state.into();
        store.apply_entity_operations(vec![EntityOperation::Set {
            key: util::entity_key("AccountState", &entity),
            data: entity
        }], None).unwrap();

        let first_deposit = PendingFlux {
            slot_index: 0,
            slot: U256::zero(),
            account_id: 0,
            token_id: 0,
            amount: 10,
        };
        let mut entity: Entity = first_deposit.into();
        entity.set("id", "1");
        store.apply_entity_operations(vec![EntityOperation::Set {
            key: util::entity_key("Deposit", &entity),
            data: entity
        }], None).unwrap();

        let second_deposit = PendingFlux {
            slot_index: 1,
            slot: U256::zero(),
            account_id: 1,
            token_id: 0,
            amount: 10,
        };
        let mut entity: Entity = second_deposit.into();
        entity.set("id", "2");
        store.apply_entity_operations(vec![EntityOperation::Set {
            key: util::entity_key("Deposit", &entity),
            data: entity
        }], None).unwrap();

        // Process event
        let log = create_state_transition_event(FluxTransitionType::Deposit, 1, H256::from(1), 0);
        let result = handler.process_event(
            util::test::logger(), 
            Arc::new(util::test::fake_block()),
            Arc::new(util::test::fake_tx()), 
            log
        );
        let expected_new_state = AccountState::new(H256::from(1), U256::one(), vec![10, 10, 0, 0], 1);

        assert!(result.is_ok());
        match result.unwrap().pop().unwrap() {
            EntityOperation::Set { key: _, data } => assert_eq!(AccountState::from(data), expected_new_state),
            _ => assert!(false)
        }
    }

    #[test]
    fn test_fails_if_state_does_not_exist() {
        let schema = util::test::fake_schema();
        let store = Arc::new(MockStore::new(vec![(schema.id.clone(), schema)]));
        let handler = FluxTransitionHandler::new(store.clone());

        // No data in store

        let log = create_state_transition_event(FluxTransitionType::Deposit, 1, H256::from(1), 0);
        let result = handler.process_event(
            util::test::logger(), 
            Arc::new(util::test::fake_block()),
            Arc::new(util::test::fake_tx()), 
            log
        );
        assert!(result.is_err());
    }

    #[test]
    fn test_applies_withdraws_existing_state() {
        let schema = util::test::fake_schema();
        let store = Arc::new(MockStore::new(vec![(schema.id.clone(), schema)]));
        let handler = FluxTransitionHandler::new(store.clone());

        // Add previous account state and pending deposits into Store
        let existing_state = AccountState::new(
            H256::zero(),
            U256::zero(),
            vec![10, 20, 0, 0],
            1
        );
        let entity = existing_state.into();
        store.apply_entity_operations(vec![EntityOperation::Set {
            key: util::entity_key("AccountState", &entity),
            data: entity
        }], None).unwrap();
        let first_withdraw = PendingFlux {
            slot_index: 0,
            slot: U256::zero(),
            account_id: 0,
            token_id: 0,
            amount: 10,
        };
        let mut entity: Entity = first_withdraw.into();
        entity.set("id", "1");
        store.apply_entity_operations(vec![EntityOperation::Set {
            key: util::entity_key("Withdraw", &entity),
            data: entity
        }], None).unwrap();

        let second_withdraw = PendingFlux {
            slot_index: 1,
            slot: U256::zero(),
            account_id: 1,
            token_id: 0,
            amount: 10,
        };
        let mut entity: Entity = second_withdraw.into();
        entity.set("id", "2");
        store.apply_entity_operations(vec![EntityOperation::Set {
            key: util::entity_key("Withdraw", &entity),
            data: entity
        }], None).unwrap();

        // Process event
        let log = create_state_transition_event(
            FluxTransitionType::Withdraw,
            1,
            H256::from(1),
            0
        );
        let result = handler.process_event(
            util::test::logger(),
            Arc::new(util::test::fake_block()),
            Arc::new(util::test::fake_tx()),
            log
        );
        let expected_new_state = AccountState::new(
            H256::from(1),
            U256::one(),
            vec![0, 10, 0, 0],
            1
        );

        assert!(result.is_ok());
        match result.unwrap().pop().unwrap() {
            EntityOperation::Set { key: _, data } => assert_eq!(AccountState::from(data), expected_new_state),
            _ => assert!(false)
        }
    }

    fn create_state_transition_event(
        transition_type: FluxTransitionType, 
        new_state_index: u8, 
        new_state_root: H256,
        applied_slot: u8
    ) -> Arc<Log> {
        let bytes: Vec<Vec<u8>> = vec![
            /* transition_type */ vec![0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, transition_type as u8],
            /* new_state_index */ vec![ 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, new_state_index],
            /* new_state_hash */ new_state_root[..].to_vec(),
            /* applied_slot */ vec![0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, applied_slot],
        ];

        Arc::new(Log {
            address: 1.into(),
            topics: vec![],
            data: Bytes(bytes.iter().flat_map(|i| i.iter()).cloned().collect()),
            block_hash: Some(2.into()),
            block_number: Some(1.into()),
            transaction_hash: Some(3.into()),
            transaction_index: Some(0.into()),
            log_index: Some(0.into()),
            transaction_log_index: Some(0.into()),
            log_type: None,
            removed: None,
        })
    }
}