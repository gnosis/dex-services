mod eth_node;
mod ethgasstation;
mod gasnow;
mod gnosis_safe;
mod linear_interpolation;
mod priority;

use crate::{contracts::Web3, http::HttpFactory};
use anyhow::Result;
use std::{sync::Arc, time::Duration};

pub const DEFAULT_GAS_LIMIT: f64 = 21000.0;
pub const DEFAULT_TIME_LIMIT: Duration = Duration::from_secs(30);

#[cfg_attr(test, mockall::automock)]
#[async_trait::async_trait]
pub trait GasPriceEstimating: Send + Sync {
    /// Estimate the gas price for a transaction to be mined "quickly".
    async fn estimate(&self) -> Result<f64> {
        self.estimate_with_limits(DEFAULT_GAS_LIMIT, DEFAULT_TIME_LIMIT)
            .await
    }
    /// Estimate the gas price for a transaction that uses <gas> to be mined within <time_limit>.
    async fn estimate_with_limits(&self, gas_limit: f64, time_limit: Duration) -> Result<f64>;
}

/// Creates the default gas price estimator for the given network.
pub async fn create_estimator(
    http_factory: &HttpFactory,
    web3: &Web3,
) -> Result<Arc<dyn GasPriceEstimating>> {
    let network_id = web3.net().version().await?;
    let mut estimators = Vec::<Box<dyn GasPriceEstimating>>::new();

    if network_id == "1" {
        let gasnow = gasnow::GasNow::new(http_factory)?;
        estimators.push(Box::new(gasnow));

        let ethgasstation = ethgasstation::EthGasStation::new(http_factory)?;
        estimators.push(Box::new(ethgasstation));
    }

    if let Some(gnosis_url) = gnosis_safe::api_url_from_network_id(&network_id) {
        let gnosis_estimator = gnosis_safe::GnosisSafeGasStation::new(http_factory, gnosis_url)?;
        estimators.push(Box::new(gnosis_estimator));
    }

    estimators.push(Box::new(web3.clone()));

    Ok(Arc::new(priority::PriorityGasPrice::new(estimators)))
}
