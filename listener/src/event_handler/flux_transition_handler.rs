use super::*;

use dfusion_core::models::util::{PopFromLogData, ToValue};
use dfusion_core::database::DbInterface;

use graph::data::store::Entity;
use std::fmt;
use web3::types::{H256, U256};

#[derive(Clone)]
pub struct FluxTransitionHandler {
    store: Arc<DbInterface>,
}

impl FluxTransitionHandler {
    pub fn new(store: Arc<DbInterface>) -> Self {
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

        let mut account_state = self.store
            .get_balances_for_state_index(&state_index)
            .map_err(|e| failure::err_msg(format!("{}", e)))?;

        match transition_type {
            FluxTransitionType::Deposit => {
                let deposits = self.store
                    .get_deposits_of_slot(&slot)
                    .map_err(|e| failure::err_msg(format!("{}", e)))?;
                account_state.apply_deposits(&deposits);
            },
            FluxTransitionType::Withdraw => {
                let withdraws = self.store
                    .get_withdraws_of_slot(&slot)
                    .map_err(|e| failure::err_msg(format!("{}", e)))?;
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
    use dfusion_core::database::tests::DbInterfaceMock;
    use dfusion_core::database::*;
    use dfusion_core::models::{PendingFlux, AccountState};
    use web3::types::{Bytes, H256};

    #[test]
    fn test_applies_deposits_existing_state() {
        let store = Arc::new(DbInterfaceMock::new());

        // Add previous account state and pending deposits into Store
        let existing_state = AccountState::new(H256::zero(), U256::zero(), vec![0, 0, 0, 0], 1);
        store.get_balances_for_state_index
            .given(U256::zero())
            .will_return(Ok(existing_state));

        let first_deposit = PendingFlux {
            slot_index: 0,
            slot: U256::zero(),
            account_id: 0,
            token_id: 0,
            amount: 10,
        };

        let second_deposit = PendingFlux {
            slot_index: 1,
            slot: U256::zero(),
            account_id: 1,
            token_id: 0,
            amount: 10,
        };
        store.get_deposits_of_slot
            .given(U256::zero())
            .will_return(Ok(vec![first_deposit, second_deposit]));

        // Process event
        let handler = FluxTransitionHandler::new(store);
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
    fn test_apply_deposit_fails_if_state_does_not_exist() {
        let store = Arc::new(DbInterfaceMock::new());

        // No data in store
        store.get_balances_for_state_index
            .given(U256::zero())
            .will_return(Err(DatabaseError::new(ErrorKind::StateError, "No State found")));

        let handler = FluxTransitionHandler::new(store);
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
        let store = Arc::new(DbInterfaceMock::new());

        // Add previous account state and pending withdraws into Store
        let existing_state = AccountState::new(
            H256::zero(),
            U256::zero(),
            vec![10, 20, 0, 0],
            1
        );
        store.get_balances_for_state_index
            .given(U256::zero())
            .will_return(Ok(existing_state));

        let first_withdraw = PendingFlux {
            slot_index: 0,
            slot: U256::zero(),
            account_id: 0,
            token_id: 0,
            amount: 10,
        };
        let second_withdraw = PendingFlux {
            slot_index: 1,
            slot: U256::zero(),
            account_id: 1,
            token_id: 0,
            amount: 10,
        };
        let invalid_withdraw = PendingFlux {
            slot_index: 1,
            slot: U256::zero(),
            account_id: 1,
            token_id: 1,
            amount: 10,
        };
        
        store.get_withdraws_of_slot
            .given(U256::zero())
            .will_return(Ok(vec![
                first_withdraw,
                second_withdraw,
                invalid_withdraw,
            ]));

        // Process event
        let handler = FluxTransitionHandler::new(store);
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