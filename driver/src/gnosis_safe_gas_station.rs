//! Retrieves gas prices from the https://safe-relay.gnosis.io/ api call `/v1/gas-station`.

use anyhow::Result;
use ethcontract::U256;
use isahc::http::uri::Uri;
use isahc::prelude::*;
use serde::Deserialize;
use std::time::Duration;
use uint::FromDecStrErr;

/// The default uri at which the gas station api is available under.
pub const DEFAULT_URI: &str = "https://safe-relay.gnosis.io/api/v1/gas-station/";

/// Result of the api call. Prices are in wei.
#[derive(Deserialize, Debug, Eq, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct GasPrice {
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

#[cfg_attr(test, mockall::automock)]
pub trait GasPriceEstimating {
    /// Retrieves gas prices from the Gnosis Safe Relay api.
    fn estimate_gas_price(&self) -> Result<GasPrice>;
}

pub struct GasStation {
    client: HttpClient,
    uri: Uri,
}

impl GasStation {
    pub fn new(request_timeout: Duration, api_uri: &str) -> Result<GasStation> {
        let client = HttpClient::builder().timeout(request_timeout).build()?;
        let uri: Uri = api_uri.parse()?;
        Ok(GasStation { client, uri })
    }
}

impl GasPriceEstimating for GasStation {
    fn estimate_gas_price(&self) -> Result<GasPrice> {
        Ok(self.client.get(&self.uri)?.json()?)
    }
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
        let expected = GasPrice {
            last_update: "2020-02-13T09:37:45.551231Z".to_string(),
            lowest: U256::from(6u64),
            safe_low: U256::from(9_000_000_001u64),
            standard: U256::from(12_000_000_001u64),
            fast: U256::from(20_000_000_001u64),
            fastest: U256::from(1_377_000_000_001u64),
        };
        assert_eq!(serde_json::from_str::<GasPrice>(json).unwrap(), expected);
    }

    #[test]
    #[ignore]
    fn real_request() {
        let gas_station = GasStation::new(Duration::from_secs(10), DEFAULT_URI).unwrap();
        let gas_price = gas_station.get_gas_price().unwrap();
        println!("{:?}", gas_price);
    }
}
