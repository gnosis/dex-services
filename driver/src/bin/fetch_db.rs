#[macro_use]
extern crate serde_derive;

mod models;

extern crate serde_json;
extern crate serde;
extern crate mongodb;
use std::io;
use mongodb::bson;
use mongodb::{Client, ThreadedClient};
use mongodb::db::ThreadedDatabase;

fn get_current_balances() -> Result<models::State, io::Error>{

    let client = Client::connect("localhost", 27017)
        .expect("Failed to initialize standalone client.");

    let coll = client.db("dfusion").collection("CurrentState");

    // Find the document and receive a cursor
    let cursor = coll.find(None, None)
        .ok().expect("Failed to execute find.");
    
    let docs: Vec<_> = cursor.map(|doc| doc.unwrap()).collect();

    if docs.len() !=1 {
        println!("Error: There should be only one CurrentState");
    }

    let json: serde_json::Value = serde_json::to_value(&docs[0]).expect("Failed to parse json");;
    let mut query=String::from(r#" { "curState": "#);
    let t=json["CurrentState"].to_string();
    query.push_str( &t );
    query.push_str(" }");
    let v: serde_json::Value = serde_json::from_str(&query).expect("Failed to parse query to serde_json::value");
    let bson = v.into();
    let mut _temp: bson::ordered::OrderedDocument = mongodb::from_bson(bson).expect("Failed to convert bson to document");
    
    let coll = client.db("dfusion").collection("State");

    let cursor = coll.find(Some(_temp) , None)
        .ok().expect("Failed to execute find.");

    let docs: Vec<_> = cursor.map(|doc| doc.unwrap()).collect();

    let json: String = serde_json::to_string(&docs[0]).expect("Failed to parse json");

    let deserialized: models::State = serde_json::from_str(&json).unwrap();
    Ok(deserialized)
}


fn get_deposits_of_slot(slot: i32) -> Result<Vec< models::Deposits >, io::Error>{

    let client = Client::connect("localhost", 27017)
        .expect("Failed to initialize standalone client.");

    let mut query=String::from(r#" { "slot": "#);
    let t=slot.to_string();
    query.push_str( &t );
    query.push_str(" }");
    let v: serde_json::Value = serde_json::from_str(&query).expect("Failed to parse query to serde_json::value");
    let bson = v.into();
    let mut _temp: bson::ordered::OrderedDocument = mongodb::from_bson(bson).expect("Failed to convert bson to document");
    
    let coll = client.db("dfusion").collection("Deposits");

    let cursor = coll.find(Some(_temp) , None)
        .ok().expect("Failed to execute find.");

    let docs: Vec<_> = cursor.map(|doc| doc.unwrap())
                                .map(|doc| serde_json::to_string(&doc)
                                    .map(|json| serde_json::from_str(&json).unwrap())
                                        .expect("Failed to parse json")).collect();
    Ok(docs)
}


fn main() {
    
    let mut state = get_current_balances().expect("Could not get the current state of the chain");
    println!("Current balances are: {:?}", state.balances);
    let deposits = get_deposits_of_slot(state.slot + 1).unwrap();
    println!("Current deposit hash: {:?}", deposits[0].depositHash);

}
