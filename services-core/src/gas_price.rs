use crate::{contracts::Web3, http::HttpClient, http::HttpFactory, metrics::HttpLabel};
use anyhow::{anyhow, Result};
use gas_estimation::{EthGasStation, GasNow, GnosisSafeGasStation, PriorityGasPrice, Transport};
use isahc::http::uri::Uri;
use serde::de::DeserializeOwned;
use std::{str::FromStr, sync::Arc};

pub use gas_estimation::GasPriceEstimating;

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
                estimators.push(Box::new(EthGasStation::new(http_factory.create()?)))
            }
            GasEstimatorType::GasNow => {
                if !is_mainnet(&network_id) {
                    return Err(anyhow!("GasNow only supports mainnet"));
                }
                estimators.push(Box::new(GasNow::new(http_factory.create()?)))
            }
            GasEstimatorType::GnosisSafe => estimators.push(Box::new(
                GnosisSafeGasStation::with_network_id(&network_id, http_factory.create()?)?,
            )),
            GasEstimatorType::Web3 => estimators.push(Box::new(web3.clone())),
        }
    }
    Ok(Arc::new(PriorityGasPrice::new(estimators)))
}

fn is_mainnet(network_id: &str) -> bool {
    network_id == "1"
}

#[cfg(test)]
use std::time::Duration;
#[cfg(test)]
mockall::mock! {
    pub GasPriceEstimating {}

    #[async_trait::async_trait]
    trait GasPriceEstimating {
        async fn estimate(&self) -> Result<f64>;
        async fn estimate_with_limits(&self, gas_limit: f64, time_limit: Duration) -> Result<f64>;
    }
}
