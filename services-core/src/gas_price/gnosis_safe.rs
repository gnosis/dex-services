//! Gnosis Safe gas station `GasPriceEstimating` implementation.
//! Api documentation at https://safe-relay.gnosis.io/ .

use super::{linear_interpolation, GasPriceEstimating};
use crate::http::{HttpClient, HttpFactory, HttpLabel};
use anyhow::Result;
use ethcontract::U256;
use isahc::http::uri::Uri;
use pricegraph::num;
use serde::Deserialize;
use serde_with::rust::display_fromstr;
use std::{convert::TryInto, time::Duration};

/// The default uris at which the gas station api is available under.
const DEFAULT_MAINNET_URI: &str = "https://safe-relay.gnosis.io/api/v1/gas-station/";
const DEFAULT_RINKEBY_URI: &str = "https://safe-relay.rinkeby.gnosis.io/api/v1/gas-station/";

pub fn api_url_from_network_id(network_id: &str) -> Option<&'static str> {
    match network_id {
        "1" => Some(DEFAULT_MAINNET_URI),
        "4" => Some(DEFAULT_RINKEBY_URI),
        _ => None,
    }
}

/// Gas prices in wei retrieved from the gas station. This is a result from the
/// API call.
#[derive(Deserialize, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct GasPrices {
    pub last_update: String,
    #[serde(with = "display_fromstr")]
    pub lowest: f64,
    #[serde(with = "display_fromstr")]
    pub safe_low: f64,
    #[serde(with = "display_fromstr")]
    pub standard: f64,
    #[serde(with = "display_fromstr")]
    pub fast: f64,
    #[serde(with = "display_fromstr")]
    pub fastest: f64,
}

const FAST_PERCENTILE: f64 = 0.75;
const STANDARD_PERCENTILE: f64 = 0.5;
const SAFE_LOW_PERCENTILE: f64 = 0.3;

const SECONDS_PER_BLOCK: f64 = 15.0;
// Treat percentiles as probabilities for geometric distribution.
const FAST_TIME: f64 = SECONDS_PER_BLOCK * 1.0 / FAST_PERCENTILE;
const STANDARD_TIME: f64 = SECONDS_PER_BLOCK * 1.0 / STANDARD_PERCENTILE;
const SAFE_LOW_TIME: f64 = SECONDS_PER_BLOCK * 1.0 / SAFE_LOW_PERCENTILE;

/// Retrieve gas prices from the Gnosis Safe gas station service.
#[derive(Debug)]
pub struct GnosisSafeGasStation {
    client: HttpClient,
    uri: Uri,
}

impl GnosisSafeGasStation {
    pub fn new(http_factory: &HttpFactory, api_uri: &str) -> Result<GnosisSafeGasStation> {
        let client = http_factory.create()?;
        let uri: Uri = api_uri.parse()?;
        Ok(GnosisSafeGasStation { client, uri })
    }

    /// Retrieves the current gas prices from the gas station.
    pub async fn gas_prices(&self) -> Result<GasPrices> {
        self.client
            .get_json_async(&self.uri, HttpLabel::GasStation)
            .await
    }
}

#[async_trait::async_trait]
impl GasPriceEstimating for GnosisSafeGasStation {
    // The default implementation calls estimate_with_limits with 30 seconds which would result in
    // the standard time instead of fast. So to keep that behavior we implement it manually.
    async fn estimate(&self) -> Result<U256> {
        let response = self.gas_prices().await?;
        Ok(num::f64_to_u256(response.fast))
    }

    async fn estimate_with_limits(&self, gas_limit: U256, time_limit: Duration) -> Result<U256> {
        let response = self.gas_prices().await?;
        let result = estimate_with_limits(&response, gas_limit, time_limit)?;
        Ok(num::f64_to_u256(result))
    }
}

fn estimate_with_limits(
    response: &GasPrices,
    _gas_limit: U256,
    time_limit: Duration,
) -> Result<f64> {
    // We insert two extra points for the linear interpolation because this gas estimator reacts
    // slowly to gas price changes which means that for example in times of rising gas prices
    // fast might not be fast enough.
    let points: &[(f64, f64)] = &[
        (0.0, response.fast * 2.0),
        (FAST_TIME, response.fast),
        (STANDARD_TIME, response.standard),
        (SAFE_LOW_TIME, response.safe_low),
        (600.0, response.safe_low / 2.0),
    ];
    Ok(linear_interpolation::interpolate(
        time_limit.as_secs_f64(),
        points.try_into()?,
    ))
}

#[cfg(test)]
pub mod tests {
    use super::super::DEFAULT_GAS_LIMIT;
    use super::*;
    use crate::util::FutureWaitExt as _;
    use assert_approx_eq::assert_approx_eq;

    #[test]
    fn deserialize() {
        let json = r#"
        {
            "lastUpdate": "2020-02-13T09:37:45.551231Z",
            "lowest": "6",
            "safeLow": "9000000001",
            "standard": "12000000001",
            "fast": "20000000001",
            "fastest": "1377000000001"
        }"#;
        let result = serde_json::from_str::<GasPrices>(json).unwrap();
        assert_eq!(result.last_update, "2020-02-13T09:37:45.551231Z");
        assert_approx_eq!(result.lowest, 6.0);
        assert_approx_eq!(result.safe_low, 9000000001.0);
        assert_approx_eq!(result.standard, 12000000001.0);
        assert_approx_eq!(result.fast, 20000000001.0);
        assert_approx_eq!(result.fastest, 1377000000001.0);
    }

    // cargo test -p services-core gnosis_safe -- --ignored --nocapture
    #[test]
    #[ignore]
    fn real_request() {
        let gas_station =
            GnosisSafeGasStation::new(&HttpFactory::default(), DEFAULT_MAINNET_URI).unwrap();
        let response = gas_station.gas_prices().wait().unwrap();
        println!("{:?}", response);
        for i in 0..10 {
            let time_limit = Duration::from_secs(i * 10);
            println!(
                "gas price estimate for {} seconds: {} gwei",
                time_limit.as_secs(),
                estimate_with_limits(&response, DEFAULT_GAS_LIMIT.into(), time_limit).unwrap(),
            );
        }
    }
}
