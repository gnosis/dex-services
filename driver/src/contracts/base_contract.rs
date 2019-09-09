use std::env;
use std::fs;

use web3::contract::Contract;
use web3::futures::Future;
use web3::types::Address;

use crate::error::DriverError;

type Result<T> = std::result::Result<T, DriverError>;
// TODO - Generalize the ContractImpl struct out from each of the two contract handlers

#[allow(dead_code)] // event_loop needs to be retained to keep web3 connection open
pub struct SmartContract {
    pub contract: Contract<web3::transports::Http>,
    pub web3: web3::Web3<web3::transports::Http>,
    event_loop: web3::transports::EventLoopHandle,
}


impl SmartContract {
    pub fn new(address: String, contract_path: String) -> Result<Self> {
        let (event_loop, transport) =
            web3::transports::Http::new(&(env::var("ETHEREUM_NODE_URL")?))?;
        let web3 = web3::Web3::new(transport);
        let contents = fs::read_to_string(contract_path)?;

        let json: serde_json::Value = serde_json::from_str(&contents)?;
        let abi: String = json
            .get("abi")
            .ok_or("No ABI for contract")?
            .to_string();
        let decoded_address = hex::decode(&address[2..])?;
        let contract_address: Address = Address::from(&decoded_address[..]);
        let contract = Contract::from_json(web3.eth(), contract_address, abi.as_bytes())?;

        Ok(SmartContract {
            contract,
            web3,
            event_loop,
        })
    }

    pub fn account_with_sufficient_balance(&self) -> Option<Address> {
        let accounts: Vec<Address> = self.web3.eth().accounts().wait().ok()?;
        accounts
            .into_iter()
            .find(|&acc| match self.web3.eth().balance(acc, None).wait() {
                Ok(balance) => !balance.is_zero(),
                Err(_) => false,
            })
    }
}
