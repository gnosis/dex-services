use failure::Error;
use slog::Logger;
use std::fmt::Debug;
use std::sync::Arc;

use graph::components::ethereum::EthereumBlock;
use graph::components::store::EntityOperation;

use web3::types::{Log, Transaction};

mod deposit_handler;
pub use deposit_handler::DepositHandler;

mod util;

pub trait EventHandler: Send + Sync + Debug {
    fn process_event(
        &self,
        logger: Logger,
        block: Arc<EthereumBlock>,
        transaction: Arc<Transaction>,
        log: Arc<Log>
    ) -> Result<Vec<EntityOperation>, Error>;
}