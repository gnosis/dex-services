use super::{linear_interpolation, GasPriceEstimating};
use crate::http::{HttpClient, HttpFactory, HttpLabel};
use anyhow::Result;
use ethcontract::U256;
use isahc::http::uri::Uri;
use pricegraph::num;
use std::time::Duration;

// Gas price estimation with https://www.gasnow.org/ , api at https://taichi.network/#gasnow .

const API_URI: &str = "https://www.gasnow.org/api/v3/gas/price";

pub struct GasNow {
    client: HttpClient,
    uri: Uri,
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

impl GasNow {
    #[allow(dead_code)]
    fn new(http_factory: &HttpFactory) -> Result<Self> {
        let client = http_factory.create()?;
        let uri = Uri::from_static(API_URI);
        Ok(Self { client, uri })
    }

    async fn gas_price(&self) -> Result<Response> {
        self.client
            .get_json_async(&self.uri, HttpLabel::GasStation)
            .await
    }
}

#[async_trait::async_trait]
impl GasPriceEstimating for GasNow {
    async fn estimate_with_limits(&self, _gas_limit: U256, time_limit: Duration) -> Result<U256> {
        let response = self.gas_price().await?.data;
        let points = &[
            (RAPID.as_secs_f64(), response.rapid),
            (FAST.as_secs_f64(), response.fast),
            (STANDARD.as_secs_f64(), response.standard),
            (SLOW.as_secs_f64(), response.slow),
        ];
        let result = linear_interpolation::interpolate(time_limit.as_secs_f64(), points);
        Ok(num::f64_to_u256(result))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::util::FutureWaitExt;

    // cargo test -p services-core gasnow -- --ignored --nocapture
    #[test]
    #[ignore]
    fn real_request() {
        let gasnow = GasNow::new(&HttpFactory::default()).unwrap();
        let response = gasnow.gas_price().wait();
        println!("{:?}", response);
    }
}
