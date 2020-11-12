mod eth_node;
mod ethgasstation;
mod gasnow;
mod gnosis_safe;
mod linear_interpolation;
mod priority;

use crate::{contracts::Web3, http::HttpClient, http::HttpFactory, metrics::HttpLabel};
use anyhow::{anyhow, Result};
use isahc::http::uri::Uri;
use serde::de::DeserializeOwned;
use std::{str::FromStr, sync::Arc, time::Duration};

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

#[async_trait::async_trait]
impl Transport for HttpClient {
    async fn get_json<'a, T: DeserializeOwned>(&self, url: &'a str) -> Result<T> {
        self.get_json_async(Uri::from_str(url)?, HttpLabel::GasStation)
            .await
    }
}

arg_enum! {
    #[derive(Debug)]
    pub enum GasEstimatorType {
        EthGasStation,
        GasNow,
        GnosisSafe,
        Web3,
    }
}

pub async fn create_priority_estimator(
    http_factory: &HttpFactory,
    web3: &Web3,
    estimator_types: &[GasEstimatorType],
) -> Result<Arc<dyn GasPriceEstimating>> {
    let network_id = web3.net().version().await?;
    let mut estimators = Vec::<Box<dyn GasPriceEstimating>>::new();
    for estimator_type in estimator_types {
        match estimator_type {
            GasEstimatorType::EthGasStation => {
                if !is_mainnet(&network_id) {
                    return Err(anyhow!("EthGasStation only supports mainnet"));
                }
                estimators.push(Box::new(ethgasstation::EthGasStation::new(
                    http_factory.create()?,
                )))
            }
            GasEstimatorType::GasNow => {
                if !is_mainnet(&network_id) {
                    return Err(anyhow!("GasNow only supports mainnet"));
                }
                estimators.push(Box::new(gasnow::GasNow::new(http_factory.create()?)))
            }
            GasEstimatorType::GnosisSafe => estimators.push(Box::new(
                gnosis_safe::GnosisSafeGasStation::with_network_id(
                    &network_id,
                    http_factory.create()?,
                )?,
            )),
            GasEstimatorType::Web3 => estimators.push(Box::new(web3.clone())),
        }
    }
    Ok(Arc::new(priority::PriorityGasPrice::new(estimators)))
}

fn is_mainnet(network_id: &str) -> bool {
    network_id == "1"
}

#[cfg(test)]
mod tests {
    use super::*;
    use isahc::ResponseExt;

    #[derive(Default)]
    pub struct TestTransport {}

    #[async_trait::async_trait]
    impl Transport for TestTransport {
        async fn get_json<'a, T: DeserializeOwned>(&self, url: &'a str) -> Result<T> {
            let json: String = isahc::get_async(Uri::from_str(url)?).await?.text()?;
            Ok(serde_json::from_str(&json)?)
        }
    }
}
