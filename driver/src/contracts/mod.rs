pub mod erc20;
pub mod stablex_auction_element;
pub mod stablex_contract;

use crate::error::DriverError;
use crate::transport::LoggingTransport;
use ethcontract::contract::MethodDefaults;
use ethcontract::{Account, PrivateKey};
use log::Level;
use std::env;
use web3::transports::{EventLoopHandle, Http};

pub type Web3 = web3::api::Web3<LoggingTransport<Http>>;

pub fn web3_provider(url: &str) -> Result<(Web3, EventLoopHandle), DriverError> {
    let (event_loop, http) = Http::new(&url)?;
    let logging = LoggingTransport::new(http, Level::Debug);
    let web3 = Web3::new(logging);

    Ok((web3, event_loop))
}

fn method_defaults(network_id: u64) -> Result<MethodDefaults, DriverError> {
    let key = {
        let private_key = env::var("PRIVATE_KEY")?;
        PrivateKey::from_hex_str(&private_key)?
    };
    let account = Account::Offline(key, Some(network_id));
    let defaults = MethodDefaults {
        from: Some(account),
        gas: None,
        gas_price: None,
    };

    Ok(defaults)
}
