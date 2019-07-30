use failure::Error;
use slog::Logger;
use std::sync::Arc;

use dfusion_core::models::PendingFlux;

use graph::components::ethereum::EthereumBlock;
use graph::components::store::EntityOperation;
use graph::data::store::{Entity};

use web3::types::{Log, Transaction};

use super::EventHandler;
use super::util;

#[derive(Debug, Clone)]
pub struct SellOrderHandler {}

impl EventHandler for SellOrderHandler {
    fn process_event(
        &self,
        logger: Logger,
        _block: Arc<EthereumBlock>,
        _transaction: Arc<Transaction>,
        log: Arc<Log>
    ) -> Result<Vec<EntityOperation>, Error> {
        let entity_id = util::entity_id_from_log(&log);
        let flux = PendingFlux::from(log);

        info!(logger, "Processing SellOrder {:?}", &flux);

        let mut entity: Entity = flux.into();
        entity.set("id", &entity_id);

        Ok(vec![
            EntityOperation::Set {
                key: util::entity_key("SellOrder", &entity),
                data: entity
            }
        ])
    }
}