mod error;

use web3::types::H256;

pub use error::*;
use super::models;

pub trait DbInterface {
    fn get_current_balances(
        &self,
        current_state_root: &H256,
    ) -> Result<models::AccountState, DatabaseError>;
    fn get_deposits_of_slot(
        &self,
        slot: u32,
    ) -> Result<Vec<models::PendingFlux>, DatabaseError>;
    fn get_withdraws_of_slot(
        &self,
        slot: u32,
    ) -> Result<Vec<models::PendingFlux>, DatabaseError>;
    fn get_orders_of_slot(
        &self,
        slot: u32,
    ) -> Result<Vec<models::Order>, DatabaseError>;
    fn get_standing_orders_of_slot(
        &self,
        slot: u32,
    ) -> Result<[models::StandingOrder; models::NUM_RESERVED_ACCOUNTS], DatabaseError>;
}