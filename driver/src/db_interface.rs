#[cfg(test)]
extern crate mock_it;

use crate::models;
use crate::error::{DriverError, ErrorKind};

use mongodb::ordered::OrderedDocument;
use mongodb::db::ThreadedDatabase;
use mongodb::{Client, ThreadedClient};

use web3::types::H256;


pub trait DbInterface {
    fn get_current_balances(
        &self,
        current_state_root: &H256,
    ) -> Result<models::State, DriverError>;
    fn get_deposits_of_slot(
        &self,
        slot: u32,
    ) -> Result<Vec<models::PendingFlux>, DriverError>;
    fn get_withdraws_of_slot(
        &self,
        slot: u32,
    ) -> Result<Vec<models::PendingFlux>, DriverError>;
    fn get_orders_of_slot(
        &self,
        slot: u32,
    ) -> Result<Vec<models::Order>, DriverError>;
}

#[derive(Clone)]
pub struct MongoDB {
    pub client: Client,
}
impl MongoDB {
    pub fn new(db_host: String, db_port: String) -> Result<MongoDB, DriverError> {
        // connect is being picked up from a trait which isn't in scope (NetworkConnector)
        // All three!
        let client = Client::connect(&db_host, db_port.parse::<u16>()?)?;
        Ok(MongoDB { client })
    }

    fn get_items_for_slot<I: From<mongodb::ordered::OrderedDocument> + std::cmp::Ord>(
        &self,
        slot: u32,
        collection: &str,
    ) -> Result<Vec<I>, DriverError> {
        let query = format!("{{ \"slot\": {:} }}", slot);
        println!("Querying {}: {}", collection, query);

        let bson = serde_json::from_str::<serde_json::Value>(&query)?.into();
        let query = mongodb::from_bson(bson)?;

        let coll = self.client.db(models::DB_NAME).collection(collection);
        let cursor = coll.find(Some(query), None)?;
        let mut docs = vec!();
        for result in cursor {
            docs.push(I::from(result?));
        } 
        docs.sort();
        Ok(docs)
    }
}

impl DbInterface for MongoDB {
    fn get_current_balances(
        &self,
        current_state_root: &H256,
    ) -> Result<models::State, DriverError> {
        let query: String = format!("{{ \"stateHash\": \"{:x}\" }}", current_state_root);
        println!("Querying stateHash: {}", query);

        let bson =  serde_json::from_str::<serde_json::Value>(&query)?.into();
        let query = mongodb::from_bson(bson)?;

        let coll = self.client.db(models::DB_NAME).collection("accounts");
        let cursor = coll.find(Some(query), None)?;
        let mut docs: Vec<OrderedDocument> = vec!();
        for result in cursor {
            docs.push(result?);
        }
        if docs.len() == 0 {
            return Err(DriverError::new(
                &format!("Expected to find a single unique state, found {}", docs.len()), ErrorKind::StateError)
            );
        }

        let json: String = serde_json::to_string(&docs[0])?;

        let deserialized: models::State = serde_json::from_str(&json)?;
        Ok(deserialized)
    }

    fn get_deposits_of_slot(
        &self,
        slot: u32,
    ) -> Result<Vec<models::PendingFlux>, DriverError> {
        self.get_items_for_slot(slot, "deposits")
    }

    fn get_withdraws_of_slot(
        &self,
        slot: u32,
    ) -> Result<Vec<models::PendingFlux>, DriverError> {
        self.get_items_for_slot(slot, "withdraws")
    }

    fn get_orders_of_slot(
        &self,
        slot: u32,
    ) -> Result<Vec<models::Order>, DriverError> {
        self.get_items_for_slot(slot, "orders")
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use mock_it::Mock;

    #[derive(Clone)]
    pub struct DbInterfaceMock {
        pub get_current_balances: Mock<H256, Result<models::State, DriverError>>,
        pub get_deposits_of_slot: Mock<u32, Result<Vec<models::PendingFlux>, DriverError>>,
        pub get_withdraws_of_slot: Mock<u32, Result<Vec<models::PendingFlux>, DriverError>>,
        pub get_orders_of_slot: Mock<u32, Result<Vec<models::Order>, DriverError>>,
    }

    impl DbInterfaceMock {
        pub fn new() -> DbInterfaceMock {
            DbInterfaceMock {
                get_current_balances: Mock::new(Err(DriverError::new("Unexpected call to get_current_balances", ErrorKind::Unknown))),
                get_deposits_of_slot: Mock::new(Err(DriverError::new("Unexpected call to get_deposits_of_slot", ErrorKind::Unknown))),
                get_withdraws_of_slot: Mock::new(Err(DriverError::new("Unexpected call to get_withdraws_of_slot", ErrorKind::Unknown))),
                get_orders_of_slot: Mock::new(Err(DriverError::new("Unexpected call to get_withdraws_of_slot", ErrorKind::Unknown))),
            }
        }
    }

    impl DbInterface for DbInterfaceMock {
        fn get_current_balances(
            &self,
            current_state_root: &H256,
        ) -> Result<models::State, DriverError> {
            self.get_current_balances.called(*current_state_root)  // https://github.com/intellij-rust/intellij-rust/issues/3164
        }
        fn get_deposits_of_slot(
            &self,
            slot: u32,
        ) -> Result<Vec<models::PendingFlux>, DriverError> {
            self.get_deposits_of_slot.called(slot)
        }
        fn get_withdraws_of_slot(
            &self,
            slot: u32,
        ) -> Result<Vec<models::PendingFlux>, DriverError> {
            self.get_withdraws_of_slot.called(slot)
        }
        fn get_orders_of_slot(
            &self,
            slot: u32,
        ) -> Result<Vec<models::Order>, DriverError> {
            self.get_orders_of_slot.called(slot)
        }
    }
}