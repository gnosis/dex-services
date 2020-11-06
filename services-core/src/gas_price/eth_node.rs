//! Ethereum node `GasPriceEstimating` implementation.

use super::GasPriceEstimating;
use crate::contracts::Web3;
use anyhow::Result;
use pricegraph::num;
use std::time::Duration;

#[async_trait::async_trait]
impl GasPriceEstimating for Web3 {
    async fn estimate_with_limits(&self, _gas_limit: f64, _time_limit: Duration) -> Result<f64> {
        self.eth()
            .gas_price()
            .await
            .map_err(From::from)
            .map(num::u256_to_f64)
    }
}
