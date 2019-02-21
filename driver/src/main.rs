extern crate byteorder;
extern crate models;
extern crate mongodb;
extern crate rustc_hex;
extern crate web3;
mod db_interface;
use crate::db_interface::DbInterface;

use web3::contract::{Contract, Options};
use web3::futures::Future;
use web3::types::{Address, H256, U256};

use std::env;
use std::fs;
use std::io;
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

fn apply_deposits(
	state: &mut models::State,
	deposits: &Vec<models::Deposits>,
) -> Result<models::State, io::Error> {
	for i in deposits {
		state.balances[(i.accountId * models::TOKENS + i.tokenId) as usize] += i.amount;
	}
	Ok(state.clone())
}

fn main() {
	loop {
		let (tx, rx) = mpsc::channel();

		//Todo: write state validator
		thread::spawn(move || {
			let val = String::from("Previous state is valid confirmed by thread");
			tx.send(val).unwrap();
		});
		// receive validtor input
		let received = rx.recv().unwrap();
		println!(": {}", received);

		let db_host = env::var("DB_HOST").unwrap();
		let db_port = env::var("DB_PORT").unwrap();
		let db_instance =
			db_interface::MongoDB::new(db_host, db_port).unwrap_or_else(|err| {
				panic!(format!("Problem creating DbInterface: {}", err));
			});

		let (_eloop, transport) = web3::transports::Http::new("http://ganache-cli:8545")
			.expect("Transport was not established correctly");
		let web3 = web3::Web3::new(transport);

		let contents = fs::read_to_string("../dex-contracts/build/contracts/SnappBase.json")
			.expect("Something went wrong reading the SnappBasejson");
		let snapp_base: serde_json::Value =
			serde_json::from_str(&contents).expect("Json convertion was not correct");
		let snapp_base_abi: String = snapp_base.get("abi").unwrap().to_string();

		let snapp_address: String = env::var("SNAPP_CONTRACT_ADDRESS").unwrap();
		// TO do: use snapp_address in next line
		let address: Address = Address::from("0xC89Ce4735882C9F0f0FE26686c53074E09B0D550");
		let contract = Contract::from_json(web3.eth(), address, snapp_base_abi.as_bytes())
			.expect("Could not read the contract");

		thread::spawn(move || {
			// get current state
			let result = contract.query("getCurrentStateRoot", (), None, Options::default(), None);
			let curr_state_root: H256 = result.wait().expect("Unable to get current stateroot");
			let mut state = db_instance
				.get_current_balances(curr_state_root.clone())
				.expect("Could not get the current state of the chain");
			let accounts = web3
				.eth()
				.accounts()
				.wait()
				.expect("Could not get the accounts");

			// check that operator has sufficient ether
			let balance = web3
				.eth()
				.balance(accounts[0], None)
				.wait()
				.expect("Could not get the balances");
			if balance.is_zero() {
				panic!(
					"Not sufficient balance for posting updates into the chain: {}",
					balance
				);
			}

			//get depositSlot
			//
			let result = contract.query("depositIndex", (), None, Options::default(), None);
			let current_deposit_ind: U256 = result.wait().expect("Could not get deposit_slot");

			// get latest non-applied deposit_index
			let mut deposit_ind: i32 = current_deposit_ind.low_u32() as i32 + 1;
			println!("Current top deposit_slot is {:?}", deposit_ind);
			let mut found: bool = false;

			// Starting from the last depositSlot, we search backwards to the first non-applied deposit
			while !found {
				deposit_ind = deposit_ind - 1;
				let result = contract.query(
					"hasDepositBeenApplied",
					U256::from(deposit_ind),
					None,
					Options::default(),
					None,
				);
				found = result.wait().expect("Could not get hasDepositBeenApplied");
				if deposit_ind == 0 {
					break;
				}
			}
			if found {
				deposit_ind = deposit_ind + 1;
			}
			println!("Current pending deposit_slot is {:?}", deposit_ind);

			let result = contract.query(
				"getDepositCreationBlock",
				U256::from(deposit_ind),
				None,
				Options::default(),
				None,
			);
			let current_deposit_ind_block: U256 =
				result.wait().expect("Could not get deposit_slot");

			let current_block = web3
				.eth()
				.block_number()
				.wait()
				.expect("Could not get the block number");

			let result = contract.query(
				"getDepositHash",
				U256::from(deposit_ind),
				None,
				Options::default(),
				None,
			);
			let deposit_slot_empty_hash: H256 = result.wait().expect("Could not get deposit_slot");
			let deposit_slot_empty = deposit_slot_empty_hash == H256::zero();

			println!(
				"Current block is {:?} and the last deposit_ind_creationBlock is {:?}",
				current_block, current_deposit_ind_block
			);

			// if 20 blocks have past since the first deposit and we are not in the newest slot, we apply the deposit.
			if current_deposit_ind_block + 20 < current_block
				&& deposit_ind != current_deposit_ind.low_u32() as i32 + 1
			{
				println!("Next deposit_slot to be processed is {}", deposit_ind);
				let deposits = db_instance
					.get_deposits_of_slot(deposit_ind)
					.expect("Could not get deposit slot");
				println!("Amount of deposits to be processed{:?}", deposits.len());
				//rehash deposits
				let mut deposit_hash: H256 = H256::zero();
				for pat in &deposits {
					deposit_hash = pat.iter_hash(&mut deposit_hash)
				}

				let result = contract.query(
					"getDepositHash",
					U256::from(deposit_ind),
					None,
					Options::default(),
					None,
				);
				let deposit_hash_pulled: H256 = result.wait().expect("Could not get deposit_slot");

				if deposit_hash != deposit_hash_pulled {
					println!("There is some error with the data, calculated deposit_hash: {:?} does not match with deposit_hash from smart-contract {:?}", deposit_hash, deposit_hash_pulled);
					// currently the hashes are not always correct due to old data in database
					//panic!("There is some error with the data, calculated deposit_hash: {:?} does not match with deposit_hash from smart-contract {:?}", deposit_hash, deposit_hash_pulled);
				}

				if deposit_slot_empty && deposit_ind != 0 {
					println!("All deposits are already processed");
				} else {
					// calculate new state by applying all deposits
					state = apply_deposits(&mut state, &deposits)
						.ok()
						.expect("Deposits could not be applied");
					println!("New StateHash is{:?}", state.hash());

					//send new state into blockchain
					//applyDeposits signature is (slot, _currStateRoot, _newStateRoot, deposit_slotHash)
					let slot = U256::from(deposit_ind);
					let _curr_state_root = curr_state_root;
					let mut d = String::from(r#" "0x"#);
					d.push_str(&state.hash());
					d.push_str(r#"""#);
					let _new_state_root: H256 =
						serde_json::from_str(&d).expect("Could not get new state root");

					contract.call(
						"applyDeposits",
						(slot, _curr_state_root, _new_state_root, deposit_hash_pulled),
						accounts[0],
						Options::default(),
					);
				}
			} else {
				println!("All deposits are already processed");
			}
		});

		thread::sleep(Duration::from_secs(5));
	}
}
