use super::*;

use failure::Error;
use slog::Logger;
use std::sync::Arc;

use dfusion_core::models::util::{PopFromLogData, ToValue};
use dfusion_core::models::{AccountState, Order, AuctionResults};

use graph::components::ethereum::EthereumBlock;
use graph::components::store::{EntityFilter, EntityOperation, Store};
use graph::data::store::{Entity};

use web3::types::{H256, U256};
use web3::types::{Log, Transaction};

use std::fmt;

use super::EventHandler;
use super::util;


#[derive(Clone)]
pub struct AuctionSettlementHandler {
    store: Arc<Store>,
}

impl AuctionSettlementHandler {
    pub fn new(store: Arc<Store>) -> Self {
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
        log: Arc<Log>
    ) -> Result<Vec<EntityOperation>, Error> {
        let mut event_data: Vec<u8> = log.data.0.clone();

        let auction_id = U256::pop_from_log_data(&mut event_data);
        let state_index = U256::pop_from_log_data(&mut event_data).saturating_sub(U256::one());
        let new_state_hash = H256::pop_from_log_data(&mut event_data);
        // Remaining information is the Auction Results
        let encoded_solution = event_data;
        info!(logger, "Received Auction Settlement Event");

        // Fetch relevant information for transition (accounts, orders, parsed solution)
        let account_query = util::entity_query(
            "AccountState", EntityFilter::Equal("stateIndex".to_string(), state_index.to_value())
        );
        let mut account_state = AccountState::from(
            self.store.find_one(account_query)?
            .ok_or_else(|| failure::err_msg(format!("No state record found for index {}", &state_index)))?
        );

        let order_query = util::entity_query(
            "SellOrders", EntityFilter::Equal("auctionId".to_string(), auction_id.to_value())
        );
        info!(logger, "Querying Orders: {:?}", order_query);

        let orders = self.store
            .find(order_query)?
            .into_iter()
            .map(Order::from)
            .collect::<Vec<Order>>();

        info!(logger, "Found {} Orders", orders.len());

        info!(logger, "Parsing auction results from {} bytes: {:?}", encoded_solution.len(), encoded_solution);
        let auction_results = AuctionResults::from(encoded_solution);

        account_state.apply_auction(&orders, auction_results);

        let mut entity: Entity = account_state.into();

        // Set the state root according to event information
        entity.set("id", new_state_hash.to_value());
        Ok(vec![
            EntityOperation::Set {
                key: util::entity_key("AccountState", &entity),
                data: entity
            }
        ])
    }
}