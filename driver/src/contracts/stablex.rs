#[cfg(test)]
extern crate mock_it;

use web3::contract::Contract;
use web3::types::{Address, H256, U128, U256};

use crate::error::DriverError;

use std::env;
use std::fs;

type Result<T> = std::result::Result<T, DriverError>;

pub trait StableXContract {
    fn get_current_auction_index(&self) -> Result<U256>;
    // TODO - auction_data should parse and return relevant orders and account balances.
    fn get_auction_data(&self, _index: u32) -> Result<H256>;

    fn submit_solution(
        &self,
        _batch_index: u32,
        _owners: Vec<H256>,
        _order_ids: Vec<u16>,
        _volumes: Vec<u128>,
        _prices: Vec<U128>,
        _token_ids_for_price: Vec<u16>,
    ) -> Result<()>;
}

#[allow(dead_code)] // event_loop needs to be retained to keep web3 connection open
pub struct StableXContractImpl {
    contract: Contract<web3::transports::Http>,
    web3: web3::Web3<web3::transports::Http>,
    event_loop: web3::transports::EventLoopHandle,
}

impl StableXContractImpl {
    pub fn new() -> Result<Self> {
        let (event_loop, transport) = web3::transports::Http::new(&(env::var("ETHEREUM_NODE_URL")?))?;
        let web3 = web3::Web3::new(transport);

        let contents = fs::read_to_string("dex-contracts/build/contracts/StablecoinConverter.json")?;
        let json: serde_json::Value = serde_json::from_str(&contents)?;
        let abi: String = json
            .get("abi")
            .ok_or("No ABI for contract")?
            .to_string();

        let contract_address = hex::decode(&(env::var("STABLE_CONTRACT_ADDRESS")?)[2..])?;
        let address: Address = Address::from(&contract_address[..]);
        let contract = Contract::from_json(web3.eth(), address, abi.as_bytes())?;

        Ok(StableXContractImpl {
            contract,
            web3,
            event_loop,
        })
    }
}

impl StableXContract for StableXContractImpl {
    fn get_current_auction_index(&self) -> Result<U256> {
        unimplemented!();
    }

    fn get_auction_data(&self, _index: u32) -> Result<H256> {
        unimplemented!();
    }

    fn submit_solution(
        &self,
        _batch_index: u32,
        _owners: Vec<H256>,
        _order_ids: Vec<u16>,
        _volumes: Vec<u128>,
        _prices: Vec<U128>,
        _token_ids_for_price: Vec<u16>,
    ) -> Result<()> {
        unimplemented!();
    }
}