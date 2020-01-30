mod error;
mod graph_reader;

use web3::types::{H256, U256};

use super::models;
pub use error::*;
pub use graph_reader::GraphReader;

use mockall::automock;

#[automock]
pub trait DbInterface: Send + Sync {
    fn get_balances_for_state_root(
        &self,
        state_root: &H256,
    ) -> Result<models::AccountState, DatabaseError>;
    fn get_balances_for_state_index(
        &self,
        state_index: &U256,
    ) -> Result<models::AccountState, DatabaseError>;
    fn get_deposits_of_slot(&self, slot: &U256) -> Result<Vec<models::PendingFlux>, DatabaseError>;
    fn get_withdraws_of_slot(&self, slot: &U256)
        -> Result<Vec<models::PendingFlux>, DatabaseError>;
    fn get_orders_of_slot(&self, slot: &U256) -> Result<Vec<models::Order>, DatabaseError>;
    fn get_standing_orders_of_slot(
        &self,
        slot: &U256,
    ) -> Result<[models::StandingOrder; models::NUM_RESERVED_ACCOUNTS], DatabaseError>;
}
