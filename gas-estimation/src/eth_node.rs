//! Ethereum node `GasPriceEstimating` implementation.

use super::GasPriceEstimating;
use anyhow::Result;
use std::time::Duration;
use web3::{types::U256, Transport, Web3};

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

// TODO(fleupold) Copied from https://github.com/paritytech/parity-common/pull/436/files
// Replace with upstream dependencey when version is released
trait FloatConversion {
    fn to_f64_lossy(self) -> f64;
}

impl FloatConversion for U256 {
    /// Lossy conversion of `U256` to `f64`.
    fn to_f64_lossy(self) -> f64 {
        let (res, factor) = match self {
            U256([_, _, 0, 0]) => (self, 1.0),
            U256([_, _, _, 0]) => (self >> 64, 2.0f64.powi(64)),
            U256([_, _, _, _]) => (self >> 128, 2.0f64.powi(128)),
        };
        (res.low_u128() as f64) * factor
    }
}
