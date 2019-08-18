use super::*;

use failure::Error;
use slog::Logger;
use std::sync::Arc;

use dfusion_core::database::DbInterface;
use dfusion_core::models::Deserializable;
use dfusion_core::models::util::{PopFromLogData, ToValue};
use dfusion_core::models::Solution;

use graph::components::ethereum::EthereumBlock;
use graph::components::store::EntityOperation;
use graph::data::store::Entity;

use web3::types::{H256, U256};
use web3::types::{Log, Transaction};

use std::fmt;

use super::EventHandler;
use super::util;


#[derive(Clone)]
pub struct AuctionSettlementHandler {
    store: Arc<DbInterface>,
}

impl AuctionSettlementHandler {
    pub fn new(store: Arc<DbInterface>) -> Self {
        AuctionSettlementHandler {
            store
        }
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
        _block: Arc<EthereumBlock>,
        _transaction: Arc<Transaction>,
        log: Arc<Log>,
    ) -> Result<Vec<EntityOperation>, Error> {
        let mut event_data: Vec<u8> = log.data.0.clone();
        info!(logger, "Parsing Auction Settlement event from {} bytes: {:?}", event_data.len(), event_data);
        let auction_id = U256::pop_from_log_data(&mut event_data);
        let state_index = U256::pop_from_log_data(&mut event_data).saturating_sub(U256::one());
        let new_state_hash = H256::pop_from_log_data(&mut event_data);

        // Strange info coming with packed bytes
        let _bytes_init = u16::pop_from_log_data(&mut event_data);
        let _byte_size = u16::pop_from_log_data(&mut event_data);

        // Remaining information is the Auction Results
        let encoded_solution = event_data;

        info!(logger, "Parsing packed Auction Results from {} bytes: {:?}", encoded_solution.len(), encoded_solution);
        let auction_results = Solution::from_bytes(encoded_solution);

        // Fetch relevant information for transition (accounts, orders, parsed solution)
        let mut account_state = self.store
            .get_balances_for_state_index(&state_index)
            .map_err(|e| failure::err_msg(format!("{}", e)))?;

        let mut orders = self.store
            .get_orders_of_slot(&auction_id)
            .map_err(|e| failure::err_msg(format!("{}", e)))?;

        let standing_orders = self.store
            .get_standing_orders_of_slot(&auction_id)
            .map_err(|e| failure::err_msg(format!("{}", e)))?;

        orders.extend(standing_orders
            .iter()
            .filter(|standing_order| standing_order.num_orders() > 0)
            .flat_map(|standing_order| standing_order.get_orders().clone())
        );
        info!(logger, "Found {} valid Orders for this auction", orders.len());

        account_state.apply_auction(&orders, auction_results);

        let mut entity: Entity = account_state.into();

        // Set the state root according to event information
        entity.set("id", new_state_hash.to_value());
        Ok(vec![
            EntityOperation::Set {
                key: util::entity_key("AccountState", &entity),
                data: entity,
            }
        ])
    }
}

//#[cfg(test)]
//pub mod unit_test {
//    use super::*;
//    use dfusion_core::database::tests::DbInterfaceMock;
//    use dfusion_core::models::{AccountState, TOKENS};
//    use graph::bigdecimal::BigDecimal;
//    use web3::types::{H256, Bytes};
//    use std::str::FromStr;
//
//    #[test]
//    fn test_from_log() {
//        let mut bytes: Vec<Vec<u8>> = vec![];
//        let mut expected_prices: Vec<u128> = vec![];
//        // Load token prices.
//        for i in 0..TOKENS {
//            bytes.push(vec![0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, i]);
//            expected_prices.push(i as u128);
//        }
//
//        bytes.push(
//            /* buy_amount_1 */ vec![0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1]
//        );
//        bytes.push(
//            /* sell_amount_1 */ vec![0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1]
//        );
//        bytes.push(
//            /* buy_amount_2 */ vec![0; 32]
//        );
//        bytes.push(
//            /* sell_amount_2 */ vec![0; 32]
//        );
//
//        let test_data: Vec<u8> = bytes.iter().flat_map(|i| i.iter()).cloned().collect();
//
//        let log = Arc::new(Log {
//            address: 1.into(),
//            topics: vec![],
//            data: Bytes(bytes.iter().flat_map(|i| i.iter()).cloned().collect()),
//            block_hash: Some(2.into()),
//            block_number: Some(1.into()),
//            transaction_hash: Some(3.into()),
//            transaction_index: Some(0.into()),
//            log_index: Some(0.into()),
//            transaction_log_index: Some(0.into()),
//            log_type: None,
//            removed: None,
//        });
//
//        let store = Arc::new(DbInterfaceMock::new());
//
//        // Add previous account state and pending deposits into Store
//        let existing_state = AccountState::new(H256::zero(), U256::zero(), vec![0, 0, 0, 0], 1);
//        store.get_balances_for_state_index
//            .given(U256::zero())
//            .will_return(Ok(existing_state));
//
//        let res = Solution::from_bytes(test_data);
//
//        let expected_buy_amounts: Vec<u128> = vec![3, 4311810048];
//        let expected_sell_amounts: Vec<u128> = vec![2, 340282366920938463463374607431768211455];
//
//
//        assert_eq!(expected_flux, PendingFlux::from(log));
//    }
//}