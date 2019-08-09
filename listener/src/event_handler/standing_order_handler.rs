use failure::Error;
use super::EventHandler;
use slog::Logger;
use std::sync::Arc;

// use dfusion_core::models::StandingOrder;

use graph::components::ethereum::EthereumBlock;
use graph::components::store::EntityOperation;
use graph::data::store::Entity;

use dfusion_core::models::StandingOrder;

use web3::types::{Log, Transaction};

use super::util;

#[derive(Debug, Clone)]
pub struct StandingOrderHandler {}

impl EventHandler for StandingOrderHandler {
    fn process_event(
        &self,
        logger: Logger,
        _block: Arc<EthereumBlock>,
        _transaction: Arc<Transaction>,
        log: Arc<Log>
    ) -> Result<Vec<EntityOperation>, Error> {
        let entity_id = util::entity_id_from_log(&log);
        let standing_order = StandingOrder::from(log);

        info!(logger, "Processing StandingOrder batch {:?}", &standing_order);
        let mut entity: Entity = standing_order.into();
        entity.set("id", &entity_id);

        // TODO: Save also the orders

        Ok(vec![
            EntityOperation::Set {
                key: util::entity_key("StandingSellOrderBatch", &entity),
                data: entity
            }
        ])
    }
}