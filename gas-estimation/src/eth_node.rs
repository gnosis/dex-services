//! Ethereum node `GasPriceEstimating` implementation.

use super::GasPriceEstimating;
use anyhow::Result;
use std::time::Duration;
use web3::{Transport, Web3};

#[async_trait::async_trait]
impl<T> GasPriceEstimating for Web3<T>
where
    T: Transport + Send + Sync,
    <T as Transport>::Out: Send,
{
    async fn estimate_with_limits(&self, _gas_limit: f64, _time_limit: Duration) -> Result<f64> {
        self.eth()
            .gas_price()
            .await
            .map_err(From::from)
            .map(|r| r.to_f64_lossy())
    }
}
