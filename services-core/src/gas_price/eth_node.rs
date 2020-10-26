//! Ethereum node `GasPriceEstimating` implementation.

use super::GasPriceEstimating;
use crate::contracts::Web3;
use anyhow::Result;
use ethcontract::U256;

#[async_trait::async_trait]
impl GasPriceEstimating for Web3 {
    async fn estimate_gas_price(&self) -> Result<U256> {
        self.eth().gas_price().await.map_err(From::from)
    }
}
