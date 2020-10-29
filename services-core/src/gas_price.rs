mod eth_node;
mod ethgasstation;
mod gasnow;
mod gnosis_safe;
mod linear_interpolation;

use crate::{contracts::Web3, http::HttpFactory};
use anyhow::Result;
use ethcontract::U256;
use std::{sync::Arc, time::Duration};

const DEFAULT_GAS_LIMIT: u32 = 21000;
const DEFAULT_TIME_LIMIT: Duration = Duration::from_secs(30);

#[cfg_attr(test, mockall::automock)]
#[async_trait::async_trait]
pub trait GasPriceEstimating: Send + Sync {
    /// Estimate the gas price for a transaction to be mined "quickly".
    async fn estimate(&self) -> Result<U256> {
        self.estimate_with_limits(DEFAULT_GAS_LIMIT.into(), DEFAULT_TIME_LIMIT)
            .await
    }
    /// Estimate the gas price for a transaction that uses <gas> to be mined within <time_limit>.
    async fn estimate_with_limits(&self, gas_limit: U256, time_limit: Duration) -> Result<U256>;
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
