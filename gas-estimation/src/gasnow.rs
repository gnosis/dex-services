use super::{linear_interpolation, GasPriceEstimating, Transport};
use anyhow::Result;
use std::{convert::TryInto, time::Duration};

// Gas price estimation with https://www.gasnow.org/ , api at https://taichi.network/#gasnow .

const API_URI: &str = "https://www.gasnow.org/api/v3/gas/price";

pub struct GasNowGasStation<T> {
    transport: T,
}

#[derive(Debug, serde::Deserialize)]
struct Response {
    code: u32,
    data: ResponseData,
}

// gas prices in wei
#[derive(Debug, serde::Deserialize)]
struct ResponseData {
    rapid: f64,
    fast: f64,
    standard: f64,
    slow: f64,
}

const RAPID: Duration = Duration::from_secs(15);
const FAST: Duration = Duration::from_secs(60);
const STANDARD: Duration = Duration::from_secs(300);
const SLOW: Duration = Duration::from_secs(600);

impl<T: Transport> GasNowGasStation<T> {
    pub fn new(transport: T) -> Self {
        Self { transport }
    }

    async fn gas_price(&self) -> Result<Response> {
        self.transport.get_json(API_URI).await
    }
}

#[async_trait::async_trait]
impl<T: Transport> GasPriceEstimating for GasNowGasStation<T> {
    async fn estimate_with_limits(&self, _gas_limit: f64, time_limit: Duration) -> Result<f64> {
        let response = self.gas_price().await?.data;
        let points: &[(f64, f64)] = &[
            (RAPID.as_secs_f64(), response.rapid),
            (FAST.as_secs_f64(), response.fast),
            (STANDARD.as_secs_f64(), response.standard),
            (SLOW.as_secs_f64(), response.slow),
        ];
        let result =
            linear_interpolation::interpolate(time_limit.as_secs_f64(), points.try_into()?);
        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::super::tests::{FutureWaitExt as _, TestTransport};
    use super::*;

    // cargo test -p services-core gasnow -- --ignored --nocapture
    #[test]
    #[ignore]
    fn real_request() {
        let gasnow = GasNowGasStation::new(TestTransport::default());
        let response = gasnow.gas_price().wait();
        println!("{:?}", response);
    }
}
