#[macro_use]
extern crate serde_derive;

extern crate serde_json;
extern crate serde;
extern crate mongodb;
use mongodb::{Bson, bson, doc};
use mongodb::{Client, ThreadedClient};
use mongodb::db::ThreadedDatabase;


fn main() {
    let client = Client::connect("localhost", 27017)
        .expect("Failed to initialize standalone client.");

    let coll = client.db("dfusion").collection("CurrentState");

    // Find the document and receive a cursor
    let mut cursor = coll.find(None, None)
        .ok().expect("Failed to execute find.");
    
    let mut cur_state; 
    // cursor.next() returns an Option<Result<Document>>
    let item = cursor.next();
    
    let cur_state = match item {
        Some(Ok(doc)) => match doc.get("CurrentState") {
            Some(&Bson::String(ref CurrentState)) => {&CurrentState;},
            _ => panic!("Expected item to be a string!"),
        },
        Some(Err(_)) => panic!("Failed to get next from server!"),
        None => panic!("Server returned no results!"),
    };

    // convert value to mongodb::bson::Document
    let json: serde_json::Value = serde_json::to_value(&cur_state).expect("Failed to parse json");
    let bson = json.into();
    let temp: bson::Document = mongodb::from_bson(bson).expect("Failed to convert bson to document");
    
    let coll = client.db("dfusion").collection("State");

    // let cursor = match coll.find(Some(doc!(cur_state) ) , None) {
    let cursor = match coll.find(Some(temp) , None) {
	    Ok(cursor) => cursor,
	    Err(error) => panic!("The following error occured: {}", error)
	};

	for doc in cursor {
	    println!("{}", doc.unwrap());       
	}
}
