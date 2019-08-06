use failure::Error;
use super::EventHandler;
use slog::Logger;
use std::sync::Arc;

// use dfusion_core::models::StandingOrder;

use graph::components::ethereum::EthereumBlock;
use graph::components::store::EntityOperation;

use web3::types::{Log, Transaction};

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
      info!(logger, "Processing StandingSellOrderBatch {:?}", "[[ Not implemented yet! ]]");
      unimplemented!();
    }
}