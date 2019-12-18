pub mod base_contract;
pub mod snapp_contract;
pub mod stablex_auction_element;
pub mod stablex_contract;

use crate::error::DriverError;
use ethcontract::{ethsign, Account, SecretKey, H256};
use std::env;
use web3::api::Web3;
use web3::transports::{EventLoopHandle, Http};

fn web3_provider() -> Result<(Web3<Http>, EventLoopHandle), DriverError> {
    let url = env::var("ETHEREUM_NODE_URL")?;
    let (event_loop, http) = Http::new(&url)?;
    let web3 = Web3::new(http);

    Ok((web3, event_loop))
}

fn default_account() -> Result<Account, DriverError> {
    let network_id = env::var("NETWORK_ID")?.parse()?;
    let secret = {
        let private_key: H256 = env::var("PRIVATE_KEY")?.parse()?;
        SecretKey::from_raw(&private_key[..]).map_err(ethsign::Error::from)?
    };
    let account = Account::Offline(secret, Some(network_id));

    Ok(account)
}
