pub mod stablex_auction_element;
pub mod stablex_contract;

use crate::transport::HttpTransport;
use anyhow::{anyhow, Error, Result};
use ethcontract::contract::MethodDefaults;
use ethcontract::errors::{ExecutionError, MethodError};

use ethcontract::{Account, PrivateKey};
use std::convert::TryFrom;
use std::fmt;
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

pub enum BenignSolutionFailure {
    BetterSolutionAlreadySubmitted,
    NegativeUtility,
}

impl fmt::Display for BenignSolutionFailure {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            BenignSolutionFailure::BetterSolutionAlreadySubmitted => {
                write!(f, "Better solution already submitted")
            }
            BenignSolutionFailure::NegativeUtility => write!(f, "Negative Utility"),
        }
    }
}

impl TryFrom<&str> for BenignSolutionFailure {
    type Error = anyhow::Error;

    fn try_from(reason: &str) -> Result<Self, Self::Error> {
        match reason {
            "New objective doesn\'t sufficiently improve current solution" => {
                Ok(BenignSolutionFailure::BetterSolutionAlreadySubmitted)
            }
            "Claimed objective doesn't sufficiently improve current solution" => {
                Ok(BenignSolutionFailure::BetterSolutionAlreadySubmitted)
            }
            "SafeMath: subtraction overflow" => Ok(BenignSolutionFailure::NegativeUtility),
            _ => Err(anyhow!("Unexpected error")),
        }
    }
}

pub fn extract_benign_submission_failure(err: &Error) -> Option<BenignSolutionFailure> {
    err.downcast_ref::<MethodError>()
        .and_then(|method_error| match &method_error.inner {
            ExecutionError::Revert(Some(reason)) => {
                let reason_slice: &str = &*reason;
                BenignSolutionFailure::try_from(reason_slice).ok()
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
