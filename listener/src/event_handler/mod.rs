use failure::Error;
use slog::Logger;
use std::fmt::Debug;
use std::sync::Arc;

use graph::components::ethereum::EthereumBlock;
use graph::components::store::EntityOperation;

use web3::types::{Log, Transaction};

mod deposit_handler;
pub use deposit_handler::DepositHandler;

mod withdraw_handler;
pub use withdraw_handler::WithdrawHandler;

mod initialization_handler;
pub use initialization_handler::InitializationHandler;

mod flux_transition_handler;
pub use flux_transition_handler::FluxTransitionHandler;

mod standing_order_handler;
pub use standing_order_handler::StandingOrderHandler;

mod order_handler;
pub use order_handler::SellOrderHandler;

mod auction_settlement_handler;
pub use auction_settlement_handler::AuctionSettlementHandler;

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