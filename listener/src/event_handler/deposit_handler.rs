use failure::Error;
use slog::Logger;
use std::sync::Arc;

use dfusion_core::models::PendingFlux;

use graph::components::ethereum::EthereumBlock;
use graph::components::store::EntityOperation;
use graph::data::store::Entity;

use web3::types::{Log, Transaction};

use super::util;
use super::EventHandler;

#[derive(Debug, Clone)]
pub struct DepositHandler {}

impl EventHandler for DepositHandler {
    fn process_event(
        &self,
        logger: Logger,
        _block: Arc<EthereumBlock>,
        _transaction: Arc<Transaction>,
        log: Arc<Log>,
    ) -> Result<Vec<EntityOperation>, Error> {
        let flux = PendingFlux::from(log);

        info!(logger, "Processing Deposit {:?}", &flux);

        let entity: Entity = flux.into();
        Ok(vec![EntityOperation::Set {
            key: util::entity_key("Deposit", &entity),
            data: entity,
        }])
    }
}
