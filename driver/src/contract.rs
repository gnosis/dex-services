use web3::contract::{Contract, Options};
use web3::futures::Future;
use web3::types::{Address, H256, U256};

use std::env;
use std::fs;

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

pub trait SnappContract {
    // General Blockchain interface
    fn get_current_block_number(&self) -> Result<U256>;

    // Top level smart contract methods
    fn get_current_state_root(&self) -> Result<H256>;
    fn get_current_deposit_slot(&self) -> Result<U256>;
    fn get_current_withdraw_slot(&self) -> Result<U256>;

    // Deposit Slots
    fn creation_block_for_deposit_slot(&self, slot: U256) -> Result<U256>;
    fn deposit_hash_for_slot(&self, slot: U256) -> Result<H256>;
    fn has_deposit_slot_been_applied(&self, slot: U256) -> Result<bool>;

    // Withdraw Slots
    fn creation_block_for_withdraw_slot(&self, slot: U256) -> Result<U256>;
    fn withdraw_hash_for_slot(&self, slot: U256) -> Result<H256>;
    fn has_withdraw_slot_been_applied(&self, slot: U256) -> Result<bool>;

    // Write methods
    fn apply_deposits(&self, slot: U256, prev_state: H256, new_state: H256, deposit_hash: H256) -> Result<()>;
    fn apply_withdraws(&self, slot: U256, merkle_root: H256, prev_state: H256, new_state: H256, withdraw_hash: H256) -> Result<()>;
}

pub struct SnappContractImpl {
    contract: Contract<web3::transports::Http>,
    web3: web3::Web3<web3::transports::Http>,
    event_loop: web3::transports::EventLoopHandle,
}

impl SnappContractImpl {
    pub fn new() -> Result<Self> {
        let (event_loop, transport) = web3::transports::Http::new("http://ganache-cli:8545")?;
        let web3 = web3::Web3::new(transport);

        let contents = fs::read_to_string("../dex-contracts/build/contracts/SnappBase.json")?;
        let snapp_base: serde_json::Value = serde_json::from_str(&contents)?;
        let snapp_base_abi: String = snapp_base.get("abi").ok_or("No ABI for contract")?.to_string();

        let snapp_address = hex::decode(env::var("SNAPP_CONTRACT_ADDRESS")?)?;
        let address: Address = Address::from(&snapp_address[..]);
        let contract = Contract::from_json(web3.eth(), address, snapp_base_abi.as_bytes())?;
        Ok(SnappContractImpl { contract, web3, event_loop })
    }

    fn account_with_sufficient_balance(&self) -> Option<Address> {
        let accounts: Vec<Address> = self.web3.eth().accounts().wait().ok()?;
        println!("{:?}", accounts[0]);
        Some(accounts[0])
        // accounts.into_iter().find(|&acc| {
        //     match self.web3.eth().balance(acc, None).wait() {
        //         Ok(balance) => !balance.is_zero(),
        //         Err(_) => false,
        //     }
        // })
    }
}

impl SnappContract for SnappContractImpl {
    fn get_current_state_root(&self) -> Result<H256> {
        self.contract.query(
            "getCurrentStateRoot", (), None, Options::default(), None
        ).wait().map_err(|e| Box::new(e) as Box<std::error::Error>)
    }

    fn get_current_deposit_slot(&self) -> Result<U256> {
        self.contract.query(
            "depositIndex", (), None, Options::default(), None
        ).wait().map_err(|e| Box::new(e) as Box<std::error::Error>)
    }

    fn has_deposit_slot_been_applied(&self, slot: U256) -> Result<bool> {
        self.contract.query(
            "hasDepositBeenApplied", slot, None, Options::default(), None,
        ).wait().map_err(|e| Box::new(e) as Box<std::error::Error>)
    }

    fn deposit_hash_for_slot(&self, slot: U256) -> Result<H256> {
        self.contract.query(
            "getDepositHash", slot, None, Options::default(), None,
        ).wait().map_err(|e| Box::new(e) as Box<std::error::Error>)
    }

    fn creation_block_for_deposit_slot(&self, slot: U256) -> Result<U256> {
        self.contract.query(
            "getDepositCreationBlock", slot, None, Options::default(), None,
        ).wait().map_err(|e| Box::new(e) as Box<std::error::Error>)
    }

    fn get_current_withdraw_slot(&self) -> Result<U256> {
        self.contract.query(
            "withdrawIndex", (), None, Options::default(), None
        ).wait().map_err(|e| Box::new(e) as Box<std::error::Error>)
    }

    fn has_withdraw_slot_been_applied(&self, slot: U256) -> Result<bool> {
        self.contract.query(
            "hasWithdrawBeenApplied", slot, None, Options::default(), None,
        ).wait().map_err(|e| Box::new(e) as Box<std::error::Error>)
    }

    fn withdraw_hash_for_slot(&self, slot: U256) -> Result<H256> {
        self.contract.query(
            "getWithdrawHash", slot, None, Options::default(), None,
        ).wait().map_err(|e| Box::new(e) as Box<std::error::Error>)
    }

    fn creation_block_for_withdraw_slot(&self, slot: U256) -> Result<U256> {
        self.contract.query(
            "getWithdrawCreationBlock", slot, None, Options::default(), None,
        ).wait().map_err(|e| Box::new(e) as Box<std::error::Error>)
    }

    fn get_current_block_number(&self) -> Result<U256> {
        self.web3.eth()
            .block_number()
            .wait()
            .map_err(|e| Box::new(e) as Box<std::error::Error>)
    }
    
    fn apply_deposits(
        &self, 
        slot: U256,
        prev_state: H256,
        new_state: H256,
        deposit_hash: H256) -> Result<()> {
            let account = self.account_with_sufficient_balance().ok_or("Not enough balance to send Txs")?;
            self.contract.call(
                "applyDeposits",
                (slot, prev_state, new_state, deposit_hash),
                account,
                Options::default(),
            ).wait()
            .map_err(|e| Box::new(e) as Box<std::error::Error>)
            .map(|_|())
    }

    fn apply_withdraws(
        &self, 
        slot: U256,
        merkle_root: H256,
        prev_state: H256,
        new_state: H256,
        withdraw_hash: H256) -> Result<()> {
            let account = self.account_with_sufficient_balance().ok_or("Not enough balance to send Txs")?;
            let mut o = Options::default();
            let s = String::from("6000000");
            o.gas = Some(U256::from_dec_str(&s).unwrap());
            println!("{:?}",o.gas);
            self.contract.call(
                "applyWithdrawals",
                (slot, merkle_root, prev_state, new_state, withdraw_hash),
                account,    
                Options::default(),
            ).wait()
            .map_err(|e| Box::new(e) as Box<std::error::Error>)
            .map(|_|())
    }
}