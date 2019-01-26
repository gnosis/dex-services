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
use std::io::{Error, ErrorKind};
use std::thread;
use std::sync::mpsc;



fn get_current_balances(client: Client, current_state_root: &H256) -> Result<models::State, Error>{
	let t: String = current_state_root.hex();
    let mut query=String::from(r#" { "stateHash": ""#);
    query.push_str( &t[2..] );
    query.push_str(r#"" }"#);

    let v: serde_json::Value = serde_json::from_str(&query).expect("Failed to parse query to serde_json::value");
    let bson = v.into();
    let mut _temp: bson::ordered::OrderedDocument = mongodb::from_bson(bson).expect("Failed to convert bson to document");
    let coll = client.db(models::DB_NAME).collection("accounts");

    let cursor = coll.find(Some(_temp) , None)
        .ok().expect("Failed to execute find.");

    let docs: Vec<_> = cursor.map(|doc| doc.unwrap()).collect();

    let json: String = serde_json::to_string(&docs[0]).expect("Failed to parse json");

    let deserialized: models::State = serde_json::from_str(&json)?;
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
    
    let coll = client.db(models::DB_NAME).collection("deposits");

    let cursor = coll.find(Some(_temp) , None)?;

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

fn get_current_state_root() -> Result<H256, io::Error> {

	let (_eloop, transport) = web3::transports::Http::new("http://localhost:8545").unwrap();
			let web3 = web3::Web3::new(transport);
	// Reading contract abi from json file.
	let contents = fs::read_to_string("./build/contracts/SnappBase.json")
	    .expect("Something went wrong reading the SnappBasejson");
	let  snapp_base: serde_json::Value  = serde_json::from_str(&contents).expect("Json convertion was not correct");
	let  snapp_base_abi: String =snapp_base.get("abi").unwrap().to_string(); 

	let address: Address  = Address::from("0xC89Ce4735882C9F0f0FE26686c53074E09B0D550");//Todo: read address .env
	let contract = Contract::from_json(web3.eth(), address ,snapp_base_abi.as_bytes()) 
 	    .expect("Could not read the contract");
	let result = contract.query("getCurrentStateRoot", (), None, Options::default(), None);
	let _curr_state_root: H256 = result.wait().unwrap();
   	Ok(_curr_state_root.clone())
} 
	
fn main() {

    loop {

	    let (tx, rx) = mpsc::channel();

	    //Todo: write state validator
	    thread::spawn(move || {
	        let val = String::from("Previous state is valid confirmed by thread");
	        tx.send(val).unwrap();

	    });

	    let received = rx.recv().unwrap();
	    println!(": {}", received);

	    thread::spawn(move || {

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
		 	    .expect("Could not read the contract");

		 	// get current state
	        let result = contract.query("getCurrentStateRoot", (), None, Options::default(), None);
			let curr_state_root: H256 = result.wait().unwrap();
		    let mut state = get_current_balances(client.clone(), &curr_state_root).expect("Could not get the current state of the chain");
		    let accounts = web3.eth().accounts().wait().expect("Could not get the accounts");

		    // check that operator has sufficient ether
		    let balance = web3.eth().balance(accounts[0], None).wait().expect("Could not get the balances");
		    if balance.is_zero() {
		    	panic!("Not sufficient balance for posting updates into the chain: {}", balance);
		    }

			//get depositSlot
			//
		   	let result = contract.query("depositIndex", (), None, Options::default(), None);
		    let current_deposit_ind: U256 = result.wait().expect("Could not get deposit_slot");
		    let current_deposit_slot: i32 = current_deposit_ind.low_u32() as i32; 
		    // get latest non-applied deposit_index 
		    let mut deposit_ind: i32 = current_deposit_ind.low_u32() as i32 + 1;
		    println!("Current top deposit_index is {:?}", deposit_ind);
		    let mut found: bool = false;

		    // Starting from the last depositSlot, we search backwards to the first non-applied deposit
		    while !found {
		    	deposit_ind = deposit_ind - 1;
		    	let result = contract.query("hasDepositBeenApplied", U256::from(deposit_ind), None, Options::default(), None);
		    	found = result.wait().expect("Could not get hasDepositBeenApplied");
		    	if deposit_ind == 0 {
		    		break;
		    	}
		    }
		    if found {
		    	deposit_ind = deposit_ind + 1;
		    }
		    println!("Current depending deposit_index is {:?}", deposit_ind);

		   	let result = contract.query("getDepositCreationBlock", (U256::from(deposit_ind)), None, Options::default(), None);
		    let current_deposit_ind_block: U256 = result.wait().expect("Could not get deposit_slot");

			println!("Current depending deposit_ind_block is {:?}", current_deposit_ind_block);

			let current_block = web3.eth().block_number().wait().expect("Could not get the block number");

			let result = contract.query("isDepositSlotEmpty", (U256::from(deposit_ind)), None, Options::default(), None);
		    let deposit_slot_empty: bool = result.wait().expect("Could not get deposit_slot");

  			println!("Current block is {:?}", current_block);

			if current_deposit_ind_block + 20 < current_block {
			    println!("Next deposit_slot to be processed is {}", deposit_ind);
			  	let deposits = get_deposits_of_slot(deposit_ind+1, client.clone()).unwrap();
			    
			    //TODO rehash deposits
			    let mut deposit_hash: H256 = H256::zero(); 
				for pat in &deposits {
					deposit_hash = pat.hash( &mut deposit_hash)
				}			    	
				println!("Current (calculated) deposit hash: {:?}", deposit_hash);

				// To be removed:
				let result = contract.query("getDepositHash", (U256::from(deposit_ind)), None, Options::default(), None);
		    	let deposit_hash: H256 = result.wait().expect("Could not get deposit_slot");

		    	println!("Current (smart-contract) deposit hash: {:?}", deposit_hash);

				if(deposit_slot_empty && deposit_ind != 0){
					println!("All deposits are already processed");
				} else {
				    // calculate new state by applying all deposits
				    state = apply_deposits(&mut state, &deposits).ok().expect("Deposits could not be applied");
				    println!("New StateHash is{:?}", state.hash());

				  	//send new state into blockchain
				  	//applyDeposits signature is (slot, _currentDeposithash, _currStateRoot, _newStateRoot)
				  	let slot = U256::from(deposit_ind);
				    let mut d=String::from(r#" ""#);
			    	d.push_str( &deposit_hash.hex() );
			    	d.push_str(r#"""#);
			   		let _current_deposithash: H256 = serde_json::from_str(&d).unwrap();
			   		let _curr_state_root = curr_state_root;
					let mut d=String::from(r#" "0x"#);
			    	d.push_str( &state.hash() );
			    	d.push_str(r#"""#);
					let _new_state_root: H256 = serde_json::from_str(&d).unwrap();
					contract.call("applyDeposits", (slot, _current_deposithash, _curr_state_root, _new_state_root), accounts[0], Options::default());
					println!("New applyDeposit was send over to anchor contract with {:?} depositHash, {:?} _new_state_root", _current_deposithash, _new_state_root);
				}
			} else {
				  	println!("All deposits are already processed");
			}
		});

	thread::sleep(Duration::from_secs(2));
	
	}
}
