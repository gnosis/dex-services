pub mod stablex_auction_element;
pub mod stablex_contract;

use crate::error::DriverError;
use crate::transport::HttpTransport;
use ethcontract::contract::MethodDefaults;
use ethcontract::{Account, PrivateKey};
use log::Level;
use std::env;
use std::time::Duration;

pub type Web3 = ethcontract::web3::api::Web3<HttpTransport>;

pub fn web3_provider(url: &str) -> Result<Web3, DriverError> {
    let http = HttpTransport::new(url, Level::Debug, Duration::from_secs(10))?;
    let web3 = Web3::new(http);

    Ok(web3)
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
