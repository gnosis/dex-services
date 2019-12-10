use clarity::PrivateKey;

use ethereum_tx_sign::RawTransaction;

use log::info;

use std::env;

use web3::contract::tokens::Tokenize;
use web3::contract::{Contract, Options};
use web3::futures::Future;
use web3::types::{Address, Bytes, H160, H256, U256};

use crate::error::{DriverError, ErrorKind};

type Result<T> = std::result::Result<T, DriverError>;

#[allow(dead_code)] // event_loop needs to be retained to keep web3 connection open
pub struct BaseContract {
    pub contract: Contract<web3::transports::Http>,
    pub web3: web3::Web3<web3::transports::Http>,
    event_loop: web3::transports::EventLoopHandle,
    abi: ethabi::Contract,
    network_id: u8,
    pub public_key: H160,
    private_key: H256,
}

impl BaseContract {
    pub fn new(address: String, contents: String) -> Result<Self> {
        let (event_loop, transport) =
            web3::transports::Http::new(&(env::var("ETHEREUM_NODE_URL")?))?;
        let web3 = web3::Web3::new(transport);

        let json: serde_json::Value = serde_json::from_str(&contents)?;
        let abi_string = json.get("abi").ok_or("No ABI for contract")?.to_string();
        let abi = abi_string.as_bytes();

        let decoded_address = hex::decode(&address[2..])?;
        let contract_address: Address = Address::from(&decoded_address[..]);
        let contract = Contract::from_json(web3.eth(), contract_address, abi)?;

        let network_id = env::var("NETWORK_ID")?.parse()?;
        let private_key = env::var("PRIVATE_KEY")?.parse()?;

        Ok(BaseContract {
            contract,
            web3,
            event_loop,
            abi: ethabi::Contract::load(abi)?,
            network_id,
            public_key: address_from_private_key(&private_key)?,
            private_key,
        })
    }

    pub fn send_signed_transaction<P>(
        &self,
        function: &str,
        params: P,
        options: Options,
    ) -> Result<H256>
    where
        P: Tokenize,
    {
        let nonce = if let Some(nonce) = options.nonce {
            nonce
        } else {
            self.web3
                .eth()
                .transaction_count(self.public_key, None)
                .wait()?
        };

        info!("Sending tx from {} with nonce {:x}", self.public_key, nonce);

        let signed_tx = self
            .abi
            .function(function)
            .and_then(|function| function.encode_input(&params.into_tokens()))
            .map(|data| {
                let tx = RawTransaction {
                    nonce,
                    to: Some(self.contract.address()),
                    value: options.value.unwrap_or_default(),
                    gas_price: options
                        .gas_price
                        .unwrap_or_else(|| U256::from(1_000_000_000)),
                    gas: options.gas.unwrap_or_else(|| U256::from(100_000)),
                    data,
                };
                tx.sign(&self.private_key, &self.network_id)
            })?;

        self.web3
            .eth()
            .send_raw_transaction(Bytes::from(signed_tx))
            .wait()
            .map_err(DriverError::from)
    }
}

fn address_from_private_key(pk: &H256) -> Result<H160> {
    let public_key = PrivateKey::from_slice(&pk.to_vec())
        .and_then(|pk| pk.to_public_key())
        .map_err(|_| {
            DriverError::new(
                "Unable to compute public key from private key",
                ErrorKind::EnvError,
            )
        })?;
    Ok(H160::from(public_key.as_bytes()))
}
