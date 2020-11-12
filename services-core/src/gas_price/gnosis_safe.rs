//! Gnosis Safe gas station `GasPriceEstimating` implementation.
//! Api documentation at https://safe-relay.gnosis.io/ .

use super::{linear_interpolation, GasPriceEstimating, Transport};

use anyhow::Result;
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

// The gnosis safe gas station looks at the gas price of all transactions in the last 200 blocks.
// The fast gas price is the price at the 75th percentile of gas prices and so on.
const FAST_PERCENTILE: f64 = 0.75;
const STANDARD_PERCENTILE: f64 = 0.5;
const SAFE_LOW_PERCENTILE: f64 = 0.3;

// For this module we need to estimate confirmation times. So we need some way of converting the
// percentiles into time. The standard percentile is 50% which means transactions at this gas price
// were included in 50% of the blocks. Thus for every block a transaction at this price has a 0.5
// chance to be included. We treat this as a geometric distribution in which the expected time for
// the event to happen (transaction is included in block) is 1/p. Blocks happen every 15 seconds so
// this is how we get a time estimate.
// In reality this estimation can be problematic when gas prices are changing quickly. When prices
// are rising, the prices calculated based on the last 200 blocks lag behind the real price. For
// this reason we insert two extra points for the linear interpolation below in estimate_limits.
// We do not make use of the lowest and fastest gas price because they are too strong outliers. The
// lowest gas price is skewed by miners including their own transactions at 1 gwei. The highest gas
// price can be 1000 times the fast gas price which is not reasonable.
const SECONDS_PER_BLOCK: f64 = 15.0;
// Treat percentiles as probabilities for geometric distribution.
const FAST_TIME: f64 = SECONDS_PER_BLOCK / FAST_PERCENTILE;
const STANDARD_TIME: f64 = SECONDS_PER_BLOCK / STANDARD_PERCENTILE;
const SAFE_LOW_TIME: f64 = SECONDS_PER_BLOCK / SAFE_LOW_PERCENTILE;

/// Retrieve gas prices from the Gnosis Safe gas station service.
#[derive(Debug)]
pub struct GnosisSafeGasStation<T> {
    transport: T,
    uri: String,
}

impl<T: Transport> GnosisSafeGasStation<T> {
    pub fn new(transport: T, uri: String) -> GnosisSafeGasStation<T> {
        GnosisSafeGasStation { transport, uri }
    }

    /// Retrieves the current gas prices from the gas station.
    pub async fn gas_prices(&self) -> Result<GasPrices> {
        self.transport.get_json(&self.uri).await
    }
}

#[async_trait::async_trait]
impl<T: Transport> GasPriceEstimating for GnosisSafeGasStation<T> {
    // The default implementation calls estimate_with_limits with 30 seconds which would result in
    // the standard time instead of fast. So to keep that behavior we implement it manually.
    async fn estimate(&self) -> Result<f64> {
        let response = self.gas_prices().await?;
        Ok(response.fast)
    }

    async fn estimate_with_limits(&self, gas_limit: f64, time_limit: Duration) -> Result<f64> {
        let response = self.gas_prices().await?;
        let result = estimate_with_limits(&response, gas_limit, time_limit)?;
        Ok(result)
    }
}

fn estimate_with_limits(
    response: &GasPrices,
    _gas_limit: f64,
    time_limit: Duration,
) -> Result<f64> {
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
    use super::super::tests::TestTransport;
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

    #[test]
    fn returns_standard_gas_price_for_30_second_limit() {
        let price = GasPrices {
            last_update: String::new(),
            lowest: 100.0,
            safe_low: 200.0,
            standard: 300.0,
            fast: 400.0,
            fastest: 500.0,
        };
        let estimate = estimate_with_limits(&price, 0.0, Duration::from_secs(30)).unwrap();
        assert_approx_eq!(estimate, 300.0);
    }

    // cargo test -p services-core gnosis_safe -- --ignored --nocapture
    #[test]
    #[ignore]
    fn real_request() {
        let gas_station =
            GnosisSafeGasStation::new(TestTransport::default(), DEFAULT_MAINNET_URI.into());
        let response = gas_station.gas_prices().wait().unwrap();
        println!("{:?}", response);
        for i in 0..10 {
            let time_limit = Duration::from_secs(i * 10);
            let price = estimate_with_limits(&response, DEFAULT_GAS_LIMIT, time_limit).unwrap();
            println!(
                "gas price estimate for {} seconds: {} gwei",
                time_limit.as_secs(),
                price / 1e9,
            );
        }
    }
}
