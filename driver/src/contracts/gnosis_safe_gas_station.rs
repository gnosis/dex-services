use ethcontract::U256;
use isahc::prelude::*;
use serde::Deserialize;
use std::time::Duration;
use uint::FromDecStrErr;

/// Result of https://safe-relay.gnosis.io/ api call `/v1/gas-station`.
///
/// Prices are in wei.
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

/// Retrieves gas prices from the Gnosis Safe Relay api.
///
/// Uses a timeout of 10 seconds.
pub fn get_gas_price() -> Result<GasPrice, isahc::Error> {
    const URL: &str = "https://safe-relay.gnosis.io/api/v1/gas-station/";
    const TIMEOUT: Duration = Duration::from_secs(10);
    // It would be more efficient to reuse the client between calls. However, we
    // only call this function once per batch when submitting a solution so this
    // is not important at the moment.
    let client = HttpClient::builder().timeout(TIMEOUT).build()?;
    client
        .get(URL)?
        .json()
        // It would more accurate to use a distinct error type but reusing this
        // avoids creating a new enum and implementing `Error` on it.
        .map_err(|err| isahc::Error::ResponseBodyError(Some(format!("{}", err))))
}

fn deserialize_u256_from_string<'de, D>(deserializer: D) -> Result<U256, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let s = String::deserialize(deserializer)?;
    U256::from_dec_str(&s)
        .map_err(uint_error_to_string)
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
}
