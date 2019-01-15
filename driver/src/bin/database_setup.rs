#[macro_use]
extern crate serde_derive;

mod global_helpers;

extern crate serde_json;
extern crate serde;
extern crate mongodb;

use mongodb::{bson, doc};
use mongodb::{Client, ThreadedClient};
use mongodb::db::ThreadedDatabase;
use pairing::{ PrimeField };


fn main() {

    let client = Client::connect("localhost", 27017)
        .expect("Failed to initialize standalone client.");

    let coll = client.db("dfusion").collection("CurrentState");

    let doc = doc! {
        "CurrentState": "0000000000000000000000000000000000000000",
    };

    // Insert document into 'dfusion.CurrentState' collection
    coll.insert_one(doc.clone(), None)
        .ok().expect("Failed to insert CurrentState.");

    let coll = client.db("dfusion").collection("State");


	let state = global_helpers::State {
	    curState: "0000000000000000000000000000000000000000".to_owned(),
    	prevState: "0000000000000000000000000000000000000000".to_owned(),
    	nextState: "0000000000000000000000000000000000000000".to_owned(),
    	slot: 0,
    	balances: [0; global_helpers::SIZE_BALANCE]
	};

    let document = serde_json::to_string(&state).ok().expect("Failed to convert first State");
    
	println!("{}", document);

    //    let document: String = String::from(r#"{"curState":"0000000000000000000000000000000000000000","prevState":"0000000000000000000000000000000000000000","nextStates":"0000000000000000000000000000000000000000","slot":0,"balances":[0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0]}"#);
    //	let temp = doc!( r#document#);
    let temp = doc! {"curState":"0000000000000000000000000000000000000000","prevState":"0000000000000000000000000000000000000000","nextStates":"0000000000000000000000000000000000000000","slot":0,"balances":[0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0]};

	 // Insert document into 'dfusion.CurrentState' collection
    coll.insert_one(temp.clone(), None)
        .ok().expect("Failed to insert CurrentState.");

    let coll = client.db("dfusion").collection("Deposits");

    let doc2 = doc! {
        "depositHash": "0000000000000000000000000000000000000000",
        "depositIndex": "0000000000000000000000000000000000000000",
        "slot": 1,
        "addressId": 1,
        "tokenId": 1,
        "amount": 55465465,
    };

    // Insert document into 'dfusion.CurrentState' collection
    coll.insert_one(doc2.clone(), None)
        .ok().expect("Failed to insert Deposit");    
}
