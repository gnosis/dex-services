//! Gnosis Safe gas station `GasPriceEstimating` implementation.

use super::GasPriceEstimating;
use crate::http::{HttpClient, HttpFactory, HttpLabel};
use anyhow::{anyhow, Result};
use ethcontract::U256;
use futures::future::{BoxFuture, FutureExt as _};
use isahc::http::uri::Uri;
use serde::Deserialize;
use uint::FromDecStrErr;

/// The default uris at which the gas station api is available under.
const DEFAULT_MAINNET_URI: &str = "https://safe-relay.gnosis.io/api/v1/gas-station/";
const DEFAULT_RINKEBY_URI: &str = "https://safe-relay.rinkeby.gnosis.io/api/v1/gas-station/";

pub fn api_url_from_network_id(network_id: u64) -> Option<&'static str> {
    match network_id {
        1 => Some(DEFAULT_MAINNET_URI),
        4 => Some(DEFAULT_RINKEBY_URI),
        _ => None,
    }
}

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

    pub fn from_network(http_factory: &HttpFactory, network_id: u64) -> Result<Self> {
        let url = api_url_from_network_id(network_id)
            .ok_or_else(|| anyhow!("no gas station configured for network {}", network_id))?;
        Self::new(http_factory, url)
    }

    /// Retrieves the current gas prices from the gas station.
    pub async fn gas_prices(&self) -> Result<GasPrices> {
        self.client
            .get_json_async(&self.uri, HttpLabel::GasStation)
            .await
    }
}

impl GasPriceEstimating for GnosisSafeGasStation {
    /// Retrieves the current gas prices from the gas station.
    fn estimate_gas_price(&self) -> BoxFuture<Result<U256>> {
        async move { Ok(self.gas_prices().await?.fast) }.boxed()
    }
}

/// Gas prices in wei retrieved from the gas station. This is a result from the
/// API call.
#[derive(Deserialize, Debug, Default, Eq, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct GasPrices {
    pub last_update: String,
    #[serde(deserialize_with = "deserialize_u256_from_string")]
    pub lowest: U256,
    #[serde(deserialize_with = "deserialize_u256_from_string")]
    pub safe_low: U256,
    #[serde(deserialize_with = "deserialize_u256_from_string")]
    pub standard: U256,
    #[serde(deserialize_with = "deserialize_u256_from_string")]
    pub fast: U256,
    #[serde(deserialize_with = "deserialize_u256_from_string")]
    pub fastest: U256,
}

fn deserialize_u256_from_string<'de, D>(deserializer: D) -> Result<U256, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let s = String::deserialize(deserializer)?;
    U256::from_dec_str(&s)
        .map_err(|err| format!("{}: {}", uint_error_to_string(err), s))
        .map_err(serde::de::Error::custom)
}

fn uint_error_to_string(err: FromDecStrErr) -> &'static str {
    match err {
        FromDecStrErr::InvalidCharacter => "FromDecStrErr: invalid character",
        FromDecStrErr::InvalidLength => "FromDecStrErr: invalid length",
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use crate::util::FutureWaitExt as _;

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
        let expected = GasPrices {
            last_update: "2020-02-13T09:37:45.551231Z".to_string(),
            lowest: U256::from(6u64),
            safe_low: U256::from(9_000_000_001u64),
            standard: U256::from(12_000_000_001u64),
            fast: U256::from(20_000_000_001u64),
            fastest: U256::from(1_377_000_000_001u64),
        };
        assert_eq!(serde_json::from_str::<GasPrices>(json).unwrap(), expected);
    }

    #[test]
    #[ignore]
    fn real_request() {
        let gas_station =
            GnosisSafeGasStation::new(&HttpFactory::default(), DEFAULT_MAINNET_URI).unwrap();
        let gas_price = gas_station.estimate_gas_price().wait().unwrap();
        println!("{:?}", gas_price);
    }
}
