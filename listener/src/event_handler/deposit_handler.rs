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
pub struct DepositHandler {}

impl EventHandler for DepositHandler {
    fn process_event(
        &self,
        logger: Logger,
        _block: Arc<EthereumBlock>,
        _transaction: Arc<Transaction>,
        log: Arc<Log>
    ) -> Result<Vec<EntityOperation>, Error> {
        let entity_id = util::entity_id_from_log(&log);
        let flux = PendingFlux::from(log);
        
        info!(logger, "Processing Deposit {:?}", &flux);
        
        let mut entity: Entity = flux.into();
        // We do not care about the ID inside the flux data model,
        // so we have to set them later.
        entity.set("id", &entity_id);
        
        Ok(vec![
            EntityOperation::Set {
                key: util::entity_key("Deposit", &entity_id),
                data: entity
            }
        ])
    }
}