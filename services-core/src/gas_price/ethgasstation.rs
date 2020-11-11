use super::{linear_interpolation, GasPriceEstimating, Transport};
use anyhow::Result;
use std::{convert::TryInto, time::Duration};

// Gas price estimation with https://ethgasstation.info/ , api https://docs.ethgasstation.info/gas-price .

const API_URI: &str = "https://ethgasstation.info/api/ethgasAPI.json";

pub struct EthGasStation<T> {
    transport: T,
}

// gas prices in gwei*10 (2 gwei is transmitted as `20`)
// wait times in minutes
#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct Response {
    fastest: f64,
    fast: f64,
    average: f64,
    safe_low: f64,
    fastest_wait: f64,
    fast_wait: f64,
    avg_wait: f64,
    safe_low_wait: f64,
}

impl<T: Transport> EthGasStation<T> {
    pub fn new(transport: T) -> Self {
        Self { transport }
    }

    async fn gas_price(&self) -> Result<Response> {
        self.transport.get_json(API_URI).await
    }
}

#[async_trait::async_trait]
impl<T: Transport> GasPriceEstimating for EthGasStation<T> {
    async fn estimate_with_limits(&self, _gas_limit: f64, time_limit: Duration) -> Result<f64> {
        let response = self.gas_price().await?;
        let result = estimate_with_limits(&response, time_limit)?;
        Ok(result)
    }
}

fn estimate_with_limits(response: &Response, time_limit: Duration) -> Result<f64> {
    let time_limit_in_minutes = time_limit.as_secs_f64() / 60.0;
    // Ethgasstation sometimes has the same time value for fastest and fast (and also gas prices
    // within 5% of eachother). This is not allowed for the linear interpolation so we filter those
    // values.
    let mut points = vec![(response.fastest_wait, response.fastest)];
    for point in &[
        (response.fast_wait, response.fast),
        (response.avg_wait, response.average),
        (response.safe_low_wait, response.safe_low),
    ] {
        if points.last().unwrap().0 < point.0 {
            points.push(*point);
        }
    }
    let gas_price_in_x10_gwei =
        linear_interpolation::interpolate(time_limit_in_minutes, points.as_slice().try_into()?);
    let gas_price_in_wei = gas_price_in_x10_gwei * 1e8;
    Ok(gas_price_in_wei)
}

#[cfg(test)]
mod tests {
    use super::super::tests::TestTransport;
    use super::*;
    use crate::util::FutureWaitExt;

    // cargo test -p services-core ethgasstation -- --ignored --nocapture
    #[test]
    #[ignore]
    fn real_request() {
        let ethgasstation = EthGasStation::new(TestTransport::default());
        let response = ethgasstation.gas_price().wait().unwrap();
        println!("{:?}", response);
        for i in 0..10 {
            let time_limit = Duration::from_secs(i * 10);
            let price = estimate_with_limits(&response, time_limit).unwrap();
            println!(
                "gas price estimate for {} seconds: {} gwei",
                time_limit.as_secs(),
                price / 1e9,
            );
        }
    }
}
