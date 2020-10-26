mod eth_node;
mod gas_station;

pub use self::gas_station::GnosisSafeGasStation;
use crate::{contracts::Web3, http::HttpFactory};
use anyhow::Result;
use ethcontract::U256;
use std::sync::Arc;

#[cfg_attr(test, mockall::automock)]
#[async_trait::async_trait]
pub trait GasPriceEstimating {
    /// Retrieves gas prices from the Gnosis Safe Relay api.
    async fn estimate_gas_price(&self) -> Result<U256>;
}

/// Creates the default gas price estimator for the given network.
pub async fn create_estimator(
    http_factory: &HttpFactory,
    web3: &Web3,
) -> Result<Arc<dyn GasPriceEstimating + Send + Sync>> {
    let network_id = web3.net().version().await?;
    Ok(match gas_station::api_url_from_network_id(&network_id) {
        Some(url) => Arc::new(GnosisSafeGasStation::new(http_factory, url)?),
        None => Arc::new(web3.clone()),
    })
}
