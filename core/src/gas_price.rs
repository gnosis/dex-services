mod eth_node;
mod gas_station;

pub use self::gas_station::GnosisSafeGasStation;
use crate::{contracts::Web3, http::HttpFactory};
use anyhow::{bail, Error, Result};
use ethcontract::U256;
use futures::future::BoxFuture;
use std::{str::FromStr, sync::Arc};

#[cfg_attr(test, mockall::automock)]
pub trait GasPriceEstimating {
    /// Retrieves gas prices from the Gnosis Safe Relay api.
    fn estimate_gas_price<'a>(&'a self) -> BoxFuture<'a, Result<U256>>;
}

/// The type of gas price estimation to use.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GasPriceEstimatingKind {
    /// Use a Gnosis Safe gas station to estimate gas prices.
    GasStation,
    /// Use the Ethereum node's gas price estimate.
    Web3,
}

impl GasPriceEstimatingKind {
    /// Creates a `GasPriceEstimating` instance.
    pub fn create(
        self,
        http_factory: &HttpFactory,
        web3: &Web3,
        network_id: u64,
    ) -> Result<Arc<dyn GasPriceEstimating + Send + Sync>> {
        Ok(match self {
            GasPriceEstimatingKind::GasStation => Arc::new(GnosisSafeGasStation::from_network(
                http_factory,
                network_id,
            )?),
            GasPriceEstimatingKind::Web3 => Arc::new(web3.clone()),
        })
    }
}

impl Default for GasPriceEstimatingKind {
    fn default() -> Self {
        GasPriceEstimatingKind::GasStation
    }
}

impl FromStr for GasPriceEstimatingKind {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self> {
        Ok(match &*s.to_lowercase() {
            "gasstation" => GasPriceEstimatingKind::GasStation,
            "web3" => GasPriceEstimatingKind::Web3,
            _ => bail!("unknown gas price estimating kind '{}'", s),
        })
    }
}
