mod eth_node;
mod ethgasstation;
mod gasnow;
mod gnosis_safe;
mod linear_interpolation;

use crate::{contracts::Web3, http::HttpFactory};
use anyhow::Result;
use std::{sync::Arc, time::Duration};

const DEFAULT_GAS_LIMIT: f64 = 21000.0;
const DEFAULT_TIME_LIMIT: Duration = Duration::from_secs(30);

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
) -> Result<Arc<dyn GasPriceEstimating + Send + Sync>> {
    let network_id = web3.net().version().await?;
    Ok(match gnosis_safe::api_url_from_network_id(&network_id) {
        Some(url) => Arc::new(gnosis_safe::GnosisSafeGasStation::new(http_factory, url)?),
        None => Arc::new(web3.clone()),
    })
}
