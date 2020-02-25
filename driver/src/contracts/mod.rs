pub mod stablex_auction_element;
pub mod stablex_contract;

use crate::transport::HttpTransport;
use anyhow::{Error, Result};
use ethcontract::contract::MethodDefaults;
use ethcontract::errors::{ExecutionError, MethodError};

use ethcontract::{Account, PrivateKey};
use std::time::Duration;

pub type Web3 = ethcontract::web3::api::Web3<HttpTransport>;

pub fn web3_provider(url: &str, timeout: Duration) -> Result<Web3> {
    let http = HttpTransport::new(url, timeout)?;
    let web3 = Web3::new(http);

    Ok(web3)
}

fn method_defaults(key: PrivateKey, network_id: u64) -> Result<MethodDefaults> {
    let account = Account::Offline(key, Some(network_id));
    let defaults = MethodDefaults {
        from: Some(account),
        gas: None,
        gas_price: None,
    };

    Ok(defaults)
}

const EXPECTED_ERRORS: &[&str; 3] = &[
    "New objective doesn\'t sufficiently improve current solution",
    "Claimed objective doesn't sufficiently improve current solution",
    "SafeMath: subtraction overflow",
];

pub fn extract_benign_submission_failure(err: &Error) -> Option<String> {
    err.downcast_ref::<MethodError>()
        .and_then(|method_error| match &method_error.inner {
            ExecutionError::Revert(Some(reason)) => {
                if EXPECTED_ERRORS.contains(&&reason[..]) {
                    Some(reason.clone())
                } else {
                    None
                }
            }
            _ => None,
        })
}

#[cfg(test)]
pub mod tests {
    use super::{ExecutionError, MethodError};
    use anyhow::anyhow;
    pub fn benign_error() -> anyhow::Error {
        anyhow!(MethodError::from_parts(
            "submitSolution(uint32,uint256,address[],uint16[],uint128[],uint128[],uint16[])"
                .to_owned(),
            ExecutionError::Revert(Some("SafeMath: subtraction overflow".to_owned())),
        ))
    }
}
