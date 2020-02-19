pub mod stablex_auction_element;
pub mod stablex_contract;

use crate::error::DriverError;
use crate::transport::HttpTransport;
use ethcontract::contract::MethodDefaults;
use ethcontract::{Account, PrivateKey};
use std::time::Duration;

pub type Web3 = ethcontract::web3::api::Web3<HttpTransport>;

pub fn web3_provider(url: &str, timeout: Duration) -> Result<Web3, DriverError> {
    let http = HttpTransport::new(url, timeout)?;
    let web3 = Web3::new(http);

    Ok(web3)
}

fn method_defaults(key: PrivateKey, network_id: u64) -> Result<MethodDefaults, DriverError> {
    let account = Account::Offline(key, Some(network_id));
    let defaults = MethodDefaults {
        from: Some(account),
        gas: None,
        gas_price: None,
    };

    Ok(defaults)
}
