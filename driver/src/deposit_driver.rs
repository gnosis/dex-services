use crate::models;
use crate::contract;
use crate::error::DriverError;

use web3::contract::{Contract, Options};
use web3::futures::Future;
use web3::types::{Address, H256, U256};

use crate::db_interface;
use crate::db_interface::DbInterface;
use std::env;
use std::error::Error;
use std::fs;

pub fn apply_deposits(
	state: &mut models::State,
	deposits: &Vec<models::Deposits>,
) -> models::State {
	for i in deposits {
		state.balances[(i.accountId * models::TOKENS + i.tokenId) as usize] += i.amount;
	}
	state.clone()
}

pub fn run_deposit_listener() -> Result<(), Box<dyn Error>> {
	let db_host = env::var("DB_HOST")?;
	let db_port = env::var("DB_PORT")?;
	let db_instance = db_interface::MongoDB::new(db_host, db_port)?;
    let contract = contract::SnappContractImpl::new()?;

	let curr_state_root: H256 = contract.get_current_state_root();
	let mut state = db_instance.get_current_balances(curr_state_root.clone())?;

	let current_deposit_ind: U256 = contract.get_current_deposit_slot();

	// get latest non-applied deposit_index
	let mut deposit_ind: i32 = current_deposit_ind.low_u32() as i32 + 1;
	println!("Current top deposit_slot is {:?}", deposit_ind);
	let mut found: bool = false;

	// Starting from the last depositSlot, we search backwards to the first non-applied deposit
	while !found {
		deposit_ind = deposit_ind - 1;
		let result = contract.has_deposit_slot_been_applied(deposit_ind);
	}
	if found {
		deposit_ind = deposit_ind + 1;
	}
	println!("Current pending deposit_slot is {:?}", deposit_ind);

	let current_deposit_ind_block = contract.creation_block_for_slot(deposit_ind)?;
	let current_block = contract.get_current_block_number()?;

	let deposit_slot_empty_hash: H256 = contract.creation_block_for_slot(deposit_ind)?;
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
		let deposits = db_instance.get_deposits_of_slot(deposit_ind)?;
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
		let deposit_hash_pulled: H256 = result.wait()?;

		if deposit_hash != deposit_hash_pulled {
			panic!("There is some error with the data, calculated deposit_hash: {:?} does not match with deposit_hash from smart-contract {:?}", deposit_hash, deposit_hash_pulled);
		}

		if deposit_slot_empty && deposit_ind != 0 {
			println!("deposit_slot {} already processed", deposit_ind);
		} else {
			// calculate new state by applying all deposits
			state = apply_deposits(&mut state, &deposits);
			println!("New StateHash is{:?}", state.hash()?);

			//send new state into blockchain
			//applyDeposits signature is (slot, _currStateRoot, _newStateRoot, deposit_slotHash)
			let slot = U256::from(deposit_ind);
			let _curr_state_root = curr_state_root;
			let mut d = String::from(r#" "0x"#);
			d.push_str(&state.hash()?);
			d.push_str(r#"""#);
			let _new_state_root: H256 = serde_json::from_str(&d)?;

			contract.apply_deposits(slot, _curr_state_root, _new_state_root, deposit_hash_pulled)?;
		}
	} else {
		println!("All deposits are already processed");
	}
	Ok(())
}
