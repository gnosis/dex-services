mod eth_node;
mod ethgasstation;
mod gasnow;
mod gnosis_safe;
mod linear_interpolation;
mod priority;

pub use ethgasstation::EthGasStation;
pub use gasnow::GasNow;
pub use gnosis_safe::GnosisSafeGasStation;
pub use priority::PriorityGasPrice;

use anyhow::Result;
use serde::de::DeserializeOwned;
use std::time::Duration;

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

#[async_trait::async_trait]
pub trait Transport: Send + Sync {
    async fn get_json<'a, T: DeserializeOwned>(&self, url: &'a str) -> Result<T>;
}

#[cfg(test)]
mod tests {
    use super::*;
    use isahc::{http::uri::Uri, ResponseExt};
    use std::{future::Future, str::FromStr};

    #[derive(Default)]
    pub struct TestTransport {}

    #[async_trait::async_trait]
    impl Transport for TestTransport {
        async fn get_json<'a, T: DeserializeOwned>(&self, url: &'a str) -> Result<T> {
            let json: String = isahc::get_async(Uri::from_str(url)?).await?.text()?;
            Ok(serde_json::from_str(&json)?)
        }
    }

    pub trait FutureWaitExt: Future + Sized {
        fn wait(self) -> Self::Output {
            futures::executor::block_on(self)
        }
    }
    impl<F> FutureWaitExt for F where F: Future {}
}
