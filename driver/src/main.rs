extern crate models;
extern crate mongodb;
extern crate tiny_keccak;
extern crate byteorder;
extern crate rustc_hex;
extern crate web3;

use web3::futures::Future;
use web3::contract::{Contract, Options};
use web3::types::{Address, H256, U256};

use std::fs;
use std::io;
use mongodb::bson;
use mongodb::{Client, ThreadedClient};
use mongodb::db::ThreadedDatabase;
use std::time::Duration;
use std::thread;


fn get_current_balances(client: Client) -> Result<models::State, io::Error>{

    let coll = client.db("dfusion").collection("CurrentState");

    // Find the document and receive a cursor
    let cursor = coll.find(None, None)
        .ok().expect("Failed to execute find.");
    
    let docs: Vec<_> = cursor.map(|doc| doc.unwrap()).collect();

    if docs.len() == 0 {
        println!("Error: No CurrentState in the dfusion database");
    }

    if docs.len() != 1 {
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


fn get_deposits_of_slot(slot: i32, client: Client) -> Result<Vec< models::Deposits >, io::Error>{

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

    let mut docs: Vec<models::Deposits> = cursor.map(|doc| doc.unwrap())
                                .map(|doc| serde_json::to_string(&doc)
                                    .map(|json| serde_json::from_str(&json).unwrap())
                                        .expect("Failed to parse json")).collect();

    docs.sort_by(|a, b| b.slot.cmp(&a.slot)); 
    Ok(docs)
}

fn apply_deposits(state: &mut models::State, deposits: &Vec<models::Deposits>) -> Result<models::State, io::Error> {
    for i in deposits {
        state.balances[ (i.accountId * models::TOKENS + i.tokenId) as usize] += i.amount;
    }
    Ok(state.clone())
}

fn main() {
    
    let client = Client::connect("localhost", 27017)
        .expect("Failed to initialize standalone client.");

	let (_eloop, transport) = web3::transports::Http::new("http://localhost:8545").unwrap();
	let web3 = web3::Web3::new(transport);
    

	// Reading contract abi from json file.
	let contents = fs::read_to_string("./build/contracts/SnappBase.json")
	    .expect("Something went wrong reading the SnappBasejson");
	let  snapp_base: serde_json::Value  = serde_json::from_str(&contents).expect("Json convertion was not correct");
	let  snapp_base_abi: String =snapp_base.get("abi").unwrap().to_string(); 

	let address: Address  = Address::from("0xC89Ce4735882C9F0f0FE26686c53074E09B0D550");//Todo: read address .env
	let contract = Contract::from_json(web3.eth(), address ,snapp_base_abi.as_bytes()) 
	    .unwrap();

    loop{
	    let mut state = get_current_balances(client.clone()).expect("Could not get the current state of the chain");
	    println!("Current balances are: {:?}", state.balances);


	    let accounts = web3.eth().accounts().wait().unwrap();
	    //Get current balance
	    let balance = web3.eth().balance(accounts[0], None).wait().unwrap();
	    if balance.is_zero() {
	    	panic!("Not sufficient balance for posting updates into the chain: {}", balance);
	    }


		 //get depositSlot
	   	let result = contract.query("depositSlot", (), None, Options::default(), None);
	    let current_deposit_ind: U256 = result.wait().unwrap();
	    let current_deposit_slot: i32 = current_deposit_ind.low_u32() as i32; 
	    // get latest non-applied deposit_index 
	    let mut deposit_ind: i32 = current_deposit_ind.low_u32() as i32 + 1;
	    println!("Current top deposit_slot is {:?}", deposit_ind);
	    let mut found: bool = false;

	    // Starting from the last depositSlot, we search backwards to the first non-applied deposit
	    while !found {
	    	deposit_ind = deposit_ind - 1;
	    	let result = contract.query("hasDepositBeenApplied", U256::from(deposit_ind), None, Options::default(), None);
	    	found = result.wait().unwrap();
	    	if deposit_ind == 0 {
	    		break;
	    	}
	    }
	    if found {
	    	deposit_ind = deposit_ind + 1;
	    }

	    // Now, we want to hop through all empty depositSlots
	    let empty_deposit_slot = true;
	    while empty_deposit_slot || current_deposit_slot == 0 {
	    	let result = contract.query("isDepositSlotEmpty", U256::from(deposit_ind), None, Options::default(), None);
	    	let empty_deposit_slot: bool = result.wait().unwrap();
	    	if !empty_deposit_slot {
	    		break;
	    	}
	    	deposit_ind = deposit_ind + 1;
	    	if deposit_ind == current_deposit_slot {
	    		break;
	    	}
	    }

	    if deposit_ind < current_deposit_slot {
		    println!("Next deposit_slot to be processed is {}", deposit_ind);
		  	let deposits = get_deposits_of_slot(deposit_ind+1, client.clone()).unwrap();
		    
		    //TODO rehash deposits
		    //deposits[0].depositHash
		    println!("Current deposit hash: {:?}", &deposits[0].depositHash);

		    // calculate new state by applying all deposits
		    state = apply_deposits(&mut state, &deposits).ok().expect("Deposits could not be applied");
		    println!("After the deposit the new balances are: {:?}", state.balances);
		    println!("New StateHash is{:?}", state.hash());

		  	//send new state into blockchain
		  	//applyDeposits signature is (slot, _currentDeposithash, _currStateRoot, _newStateRoot)
		  	let slot = U256::from(deposit_ind);
		    let mut d=String::from(r#" ""#);
	    	d.push_str( &deposits[0].depositHash );
	    	d.push_str(r#"""#);
	   		let _current_deposithash: H256 = serde_json::from_str(&d).unwrap();
	   		let result = contract.query("getCurrentStateRoot", (), None, Options::default(), None);
	    	let _curr_state_root: H256 = result.wait().unwrap();
			let mut d=String::from(r#" "0x"#);
	    	d.push_str( &state.hash() );
	    	d.push_str(r#"""#);
			let _new_state_root: H256 = serde_json::from_str(&d).unwrap();
			contract.call("applyDeposits", (slot, _current_deposithash, _curr_state_root, _new_state_root), accounts[0], Options::default());
			println!("Send new state to the anchor contract!");
		} else{
			println!("All deposits are already processed");
		}
		
		thread::sleep(Duration::from_secs(2));
	}
}
