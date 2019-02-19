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
        println!("Querying stateHash: {}", query);

        let bson =  serde_json::from_str::<serde_json::Value>(&query)?.into();
        let query = mongodb::from_bson(bson)?;

        let coll = self.client.db(models::DB_NAME).collection("accounts");
        let cursor = coll.find(Some(query), None)?;
        let mut docs: Vec<bson::ordered::OrderedDocument> = vec!();
        for result in cursor {
            docs.push(result?);
        }
        if docs.len() != 0 {
            return Err(Box::new(Error::new(
                ErrorKind::Other, format!("Expeceted to find a single unique state, found {}", docs.len()))
            ));
        }

        let json: String = serde_json::to_string(&docs[0])?;

pub fn get_deposits_of_slot<T: DbInterface>(
    db: T,
    slot: i32,
) -> Result<Vec<models::Deposits>, io::Error> {
    let mut query = String::from(r#" { "slot": "#);
    let t = slot.to_string();
    query.push_str(&t);
    query.push_str(" }");
    let v: serde_json::Value =
        serde_json::from_str(&query).expect("Failed to parse query to serde_json::value");
    let bson = v.into();
    let mut _temp: bson::ordered::OrderedDocument =
        mongodb::from_bson(bson).expect("Failed to convert bson to document");

    let cursor = db.read_data_from(models::DB_NAME, "deposits", _temp);

    let mut docs: Vec<models::Deposits> = cursor
        .map(|doc| doc.unwrap())
        .map(|doc| {
            serde_json::to_string(&doc)
                .map(|json| serde_json::from_str(&json).unwrap())
                .expect("Failed to parse json")
        })
        .collect();

    fn get_deposits_of_slot(
        &self,
        slot: i32,
    ) -> Result<Vec<models::Deposits>, Box<dyn std::error::Error>> {
        let query = format!("{{ \"slot\": {:} }}", slot);
        println!("Querying deposits: {}", query);

        let bson = serde_json::from_str::<serde_json::Value>(&query)?.into();
        let query = mongodb::from_bson(bson)?;

        let coll = self.client.db(models::DB_NAME).collection("deposits");
        let cursor = coll.find(Some(query), None)?;
        let mut docs: Vec<models::Deposits> =vec!();
        for result in cursor {
            docs.push(models::Deposits::from(result?));
        } 
        docs.sort_by(|a, b| b.slot.cmp(&a.slot));
        Ok(docs)
    }
}
