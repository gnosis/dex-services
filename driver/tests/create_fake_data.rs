
extern crate models;
extern crate serde_json;
extern crate serde;
extern crate mongodb;

use mongodb::{bson, doc};
use mongodb::{Client, ThreadedClient};
use mongodb::db::ThreadedDatabase;

#[test]
fn create_fake_data() {

    let client = Client::connect("localhost", 27017)
        .expect("Failed to initialize standalone client.");

    let coll = client.db("dfusion").collection("CurrentState");

    let doc = doc! {
        "CurrentState": "0x0000000000000000000000000000000000000000000000000000000000000000",
    };

    // Insert document into 'dfusion.CurrentState' collection
    coll.insert_one(doc.clone(), None)
        .ok().expect("Failed to insert CurrentState.");

    let coll = client.db("dfusion").collection("State");


	let state = models::State {
	    curState: "0x0000000000000000000000000000000000000000000000000000000000000000".to_owned(),
    	slot: 0,
    	balances: vec![0; models::SIZE_BALANCE],
	};

    let json: serde_json::Value = serde_json::to_value(&state).expect("Failed to parse json");
    let bson = json.into();
    let temp: bson::Document = mongodb::from_bson(bson).expect("Failed to convert bson to document");
    
    println!("Data to be inserted{:?}", temp );
	 // Insert document into 'dfusion.CurrentState' collection
    coll.insert_one(temp.clone(), None)
        .ok().expect("Failed to insert CurrentState.");

    let coll = client.db("dfusion").collection("Deposits");

    let doc2 = doc! {
        "depositHash": "0x0000000000000000000000000000000000000000000000000000000000000010",
        "slotIndex": 0,
        "slot": 1,
        "accountId": 1,
        "tokenId": 1,
        "amount": 55465465,
    };

    // Insert document into 'dfusion.CurrentState' collection
    coll.insert_one(doc2.clone(), None)
        .ok().expect("Failed to insert Deposit");    
    let doc2 = doc! {
        "depositHash": "0x0000000000000000000000000000000000000000000000000000000000000020",
        "slotIndex": 1,
        "slot": 1,
        "accountId": 1,
        "tokenId": 0,
        "amount": 65465,
    };

    // Insert document into 'dfusion.CurrentState' collection
    coll.insert_one(doc2.clone(), None)
        .ok().expect("Failed to insert Deposit");        
}
