//! Ethereum node `GasPriceEstimating` implementation.

use super::GasPriceEstimating;
use crate::contracts::Web3;
use anyhow::Result;
use ethcontract::U256;
use std::time::Duration;

#[async_trait::async_trait]
impl GasPriceEstimating for Web3 {
    async fn estimate(&self) -> Result<U256> {
        self.eth().gas_price().await.map_err(From::from)
    }

    async fn estimate_with_limits(&self, _gas_limit: U256, _time_limit: Duration) -> Result<U256> {
        self.estimate().await
    }
}
