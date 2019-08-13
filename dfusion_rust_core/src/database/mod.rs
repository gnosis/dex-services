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

pub mod tests {
    extern crate mock_it;

    use super::*;
    use mock_it::Mock;

    #[derive(Clone)]
    pub struct DbInterfaceMock {
        pub get_current_balances: Mock<H256, Result<models::AccountState, DatabaseError>>,
        pub get_deposits_of_slot: Mock<u32, Result<Vec<models::PendingFlux>, DatabaseError>>,
        pub get_withdraws_of_slot: Mock<u32, Result<Vec<models::PendingFlux>, DatabaseError>>,
        pub get_orders_of_slot: Mock<u32, Result<Vec<models::Order>, DatabaseError>>,
        pub get_standing_orders_of_slot: Mock<u32, Result<[models::StandingOrder; models::NUM_RESERVED_ACCOUNTS], DatabaseError>>,
    }

    impl DbInterfaceMock {
        pub fn new() -> DbInterfaceMock {
            DbInterfaceMock {
                get_current_balances: Mock::new(Err(DatabaseError::new(ErrorKind::Unknown, "Unexpected call to get_current_balances"))),
                get_deposits_of_slot: Mock::new(Err(DatabaseError::new(ErrorKind::Unknown, "Unexpected call to get_deposits_of_slot"))),
                get_withdraws_of_slot: Mock::new(Err(DatabaseError::new(ErrorKind::Unknown, "Unexpected call to get_withdraws_of_slot"))),
                get_orders_of_slot: Mock::new(Err(DatabaseError::new(ErrorKind::Unknown, "Unexpected call to get_withdraws_of_slot"))),
                get_standing_orders_of_slot: Mock::new(Err(DatabaseError::new(ErrorKind::Unknown, "Unexpected call to get_standing_orders_of_slot"))),
            }
        }
    }

    impl Default for DbInterfaceMock {
        fn default() -> Self {
            Self::new()
        }
    }

    impl DbInterface for DbInterfaceMock {
        fn get_current_balances(
            &self,
            current_state_root: &H256,
        ) -> Result<models::AccountState, DatabaseError> {
            self.get_current_balances.called(*current_state_root)  // https://github.com/intellij-rust/intellij-rust/issues/3164
        }
        fn get_deposits_of_slot(
            &self,
            slot: u32,
        ) -> Result<Vec<models::PendingFlux>, DatabaseError> {
            self.get_deposits_of_slot.called(slot)
        }
        fn get_withdraws_of_slot(
            &self,
            slot: u32,
        ) -> Result<Vec<models::PendingFlux>, DatabaseError> {
            self.get_withdraws_of_slot.called(slot)
        }
        fn get_orders_of_slot(
            &self,
            slot: u32,
        ) -> Result<Vec<models::Order>, DatabaseError> {
            self.get_orders_of_slot.called(slot)
        }
        fn get_standing_orders_of_slot(
            &self,
            slot: u32,
        ) -> Result<[models::StandingOrder; models::NUM_RESERVED_ACCOUNTS], DatabaseError> {
            self.get_standing_orders_of_slot.called(slot)
        }
    }
}