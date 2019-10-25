use failure::Error;
use slog::{info, Logger};
use std::sync::Arc;

use dfusion_core::models::Order;

use graph::components::ethereum::EthereumBlock;
use graph::components::store::EntityOperation;
use graph::data::store::Entity;

use web3::types::{Log, Transaction};

use super::util;
use super::EventHandler;

#[derive(Debug, Clone)]
pub struct SellOrderHandler {}

impl EventHandler for SellOrderHandler {
    fn process_event(
        &self,
        logger: Logger,
        _block: Arc<EthereumBlock>,
        _transaction: Arc<Transaction>,
        log: Arc<Log>,
    ) -> Result<Vec<EntityOperation>, Error> {
        let entity_id = util::entity_id_from_log(&log);
        let order = Order::from(log);
        info!(logger, "Processing SellOrder {:?}", &order);

        let mut entity: Entity = order.into();
        entity.set("id", &entity_id);

        Ok(vec![EntityOperation::Set {
            key: util::entity_key("SellOrder", &entity),
            data: entity,
        }])
    }
}
