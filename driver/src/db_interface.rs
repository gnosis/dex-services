use crate::models;
use crate::error::{DriverError, ErrorKind};

use mongodb::bson;
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
}

#[derive(Clone)]
pub struct MongoDB {
    pub client: Client,
}
impl MongoDB {
    pub fn new(db_host: String, db_port: String) -> Result<MongoDB, DriverError> {
        let client = Client::connect(&db_host, db_port.parse::<u16>()?)?;
        Ok(MongoDB { client })
    }

    fn get_items_for_slot(
        &self,
        slot: u32,
        collection: &str,
    ) -> Result<Vec<models::PendingFlux>, DriverError> {
        let query = format!("{{ \"slot\": {:} }}", slot);
        println!("Querying {}: {}", collection, query);

        let bson = serde_json::from_str::<serde_json::Value>(&query)?.into();
        let query = mongodb::from_bson(bson)?;

        let coll = self.client.db(models::DB_NAME).collection(collection);
        let cursor = coll.find(Some(query), None)?;
        let mut docs: Vec<models::PendingFlux> =vec!();
        for result in cursor {
            docs.push(models::PendingFlux::from(result?));
        } 
        docs.sort_by(|a, b| b.slot.cmp(&a.slot));
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
        let mut docs: Vec<bson::ordered::OrderedDocument> = vec!();
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
}