use crate::models;

use mongodb::bson;
use mongodb::db::ThreadedDatabase;
use mongodb::{Client, ThreadedClient};

use web3::types::H256;

use std::io::{Error, ErrorKind};

pub trait DbInterface {
    fn get_current_balances(
        &self,
        current_state_root: H256,
    ) -> Result<models::State, Box<dyn std::error::Error>>;
    fn get_deposits_of_slot(
        &self,
        slot: i32,
    ) -> Result<Vec<models::Deposits>, Box<dyn std::error::Error>>;
}

#[derive(Clone)]
pub struct MongoDB {
    pub client: Client,
}
impl MongoDB {
    pub fn new(db_host: String, db_port: String) -> Result<MongoDB, Box<dyn std::error::Error>> {
        let client = Client::connect(&db_host, db_port.parse::<u16>()?)?;
        Ok(MongoDB { client })
    }
}
impl DbInterface for MongoDB {
    fn get_current_balances(
        &self,
        current_state_root: H256,
    ) -> Result<models::State, Box<dyn std::error::Error>> {
        let query: String = format!("{{ \"stateHash\": \"{:x}\" }}", current_state_root);

        let v: serde_json::Value = serde_json::from_str(&query)?;
        let bson = v.into();
        let mut _temp: bson::ordered::OrderedDocument =
            mongodb::from_bson(bson).expect("Failed to convert bson to document");

        let coll = self.client.db(models::DB_NAME).collection("accounts");

        let cursor = coll.find(Some(_temp), None)?;

        let docs: Vec<bson::ordered::OrderedDocument> = cursor.map(|doc| doc.unwrap()).collect();

        if docs.len() == 0 {
            Error::new(ErrorKind::Other, "Error, state was not found");
        }

        let json: String = serde_json::to_string(&docs[0])?;

        let deserialized: models::State = serde_json::from_str(&json)?;
        Ok(deserialized)
    }

    fn get_deposits_of_slot(
        &self,
        slot: i32,
    ) -> Result<Vec<models::Deposits>, Box<dyn std::error::Error>> {
        let query = format!("{{ \"slot\": \"{:}\" }}", slot);
        
        let v: serde_json::Value =
            serde_json::from_str(&query)?;
        let bson = v.into();
        let mut _temp: bson::ordered::OrderedDocument =
            mongodb::from_bson(bson)?;

        let coll = self.client.db(models::DB_NAME).collection("deposits");

        let cursor = coll.find(Some(_temp), None)?;

        let mut docs: Vec<models::Deposits> = cursor
            .map(|doc| doc.unwrap())
            .map(|doc| {
                serde_json::to_string(&doc)
                    .map(|json| serde_json::from_str(&json).unwrap()).expect("needed here")
            })
            .collect();

        docs.sort_by(|a, b| b.slot.cmp(&a.slot));
        Ok(docs)
    }
}