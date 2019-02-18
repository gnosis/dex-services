pub mod db_interface{    
    use mongodb::bson;
    use mongodb::db::ThreadedDatabase;
    use mongodb::{Client, ThreadedClient};

    use std::env;

    
    extern crate models;
    extern crate rustc_hex;
    extern crate web3;

    use web3::types::{Address, H256, U256};

    use std::fs;
    use std::io;
    use std::io::{Error, ErrorKind};
    use std::sync::mpsc;
    use std::thread;
    use std::time::Duration;
    use std::process;
    
    #[derive(Clone)]
    pub struct DbInterface{
        pub client: Client,
        pub db_host: String,
        pub db_port: String,
    }

    impl DbInterface {
        pub fn new(db_host: String, db_port: String) -> Result<DbInterface, &'static str> {
            let client = Client::connect(&db_host, db_port.parse::<u16>().unwrap())
                .expect("Failed to initialize standalone client");

            Ok(DbInterface { client, db_host, db_port })
        }

        pub fn get_current_balances(&self, current_state_root: H256) -> Result<models::State, Error> {
            let t: String = format!("{:#x}", current_state_root);
            let mut query = String::from(r#" { "stateHash": ""#);
            query.push_str(&t[2..]);
            query.push_str(r#"" }"#);
            println!("{}", query);

            let v: serde_json::Value =
                serde_json::from_str(&query).expect("Failed to parse query to serde_json::value");
            let bson = v.into();
            let mut _temp: bson::ordered::OrderedDocument =
                mongodb::from_bson(bson).expect("Failed to convert bson to document");
            let coll = self.client.db(models::DB_NAME).collection("accounts");

            let cursor = coll
                .find(Some(_temp), None)
                .ok()
                .expect("Failed to execute find.");

            let docs: Vec<_> = cursor.map(|doc| doc.unwrap()).collect();

            if docs.len() == 0 {
                   Error::new(ErrorKind::Other, "Error, state was not found");
                   println!("here is the problem")
            }

            let json: String = serde_json::to_string(&docs[0]).expect("Failed to parse json");

            let deserialized: models::State = serde_json::from_str(&json)?;
            Ok(deserialized)
        }
        
        pub fn get_deposits_of_slot(&self, slot: i32) -> Result<Vec<models::Deposits>, io::Error> {
            let mut query = String::from(r#" { "slot": "#);
            let t = slot.to_string();
            query.push_str(&t);
            query.push_str(" }");
            let v: serde_json::Value =
                serde_json::from_str(&query).expect("Failed to parse query to serde_json::value");
            let bson = v.into();
            let mut _temp: bson::ordered::OrderedDocument =
                mongodb::from_bson(bson).expect("Failed to convert bson to document");

            let coll = self.client.db(models::DB_NAME).collection("deposits");

            let cursor = coll.find(Some(_temp), None)?;

            let mut docs: Vec<models::Deposits> = cursor
                .map(|doc| doc.unwrap())
                .map(|doc| {
                    serde_json::to_string(&doc)
                        .map(|json| serde_json::from_str(&json).unwrap())
                        .expect("Failed to parse json")
                })
                .collect();

            docs.sort_by(|a, b| b.slot.cmp(&a.slot));
            Ok(docs)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;
    use std::process;
    use web3::types::{Address, H256, U256};
    use mongodb::ThreadedClient;
    use mongodb::db::ThreadedDatabase;
    use mongodb::bson;

    #[test] 
    fn reads_balances_correctly() {

        let db_host = env::var("DB_HOST").unwrap();
        let db_port = env::var("DB_PORT").unwrap();
        let db_instance = db_interface::DbInterface::new(db_host, db_port).unwrap_or_else(|err| {
      		println!("Problem creating DbInterface: {}", err);
			process::exit(1);
    	});
        let coll = db_instance.client.db(models::DB_NAME).collection("accounts");
        let state = models::State {
            stateHash: "f00000000000000000000000000000000000000000000000000000000000000f".to_owned(),
            stateIndex: 60,
            balances: vec![5; models::SIZE_BALANCE],
        };

        let json: serde_json::Value = serde_json::to_value(&state).expect("Failed to parse json");
        let bson = json.into();
        let temp: bson::Document = mongodb::from_bson(bson).expect("Failed to convert bson to document");
        
        // Insert document into 'dfusion.CurrentState' collection
        coll.insert_one(temp.clone(), None)
        .ok().expect("Failed to insert test state");


        let d = String::from(r#" "0xf00000000000000000000000000000000000000000000000000000000000000f""#);
		let state_root: H256 =
						serde_json::from_str(&d).expect("Could not get new state root");
                        println!("{}", state_root);
        let state = db_instance.get_current_balances(state_root.clone())
		 		.expect("Could not get the current state of the chain");
        println!("Data to be inserted{:?}", state.balances );

        assert!(state.balances[5] == 5);
    }
}