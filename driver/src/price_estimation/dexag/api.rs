use anyhow::{anyhow, Context, Result};
use ethcontract::Address;
use isahc::prelude::*;
use serde::{de::Error as _, Deserialize, Deserializer};
use serde_with::rust::display_fromstr;
use std::time::Duration;
use url::Url;

#[cfg_attr(test, mockall::automock)]
/// Parts of the dex.ag api.
pub trait DexagApi {
    /// https://docs.dex.ag/api/tokens
    fn get_token_list(&self) -> Result<Vec<Token>>;
    /// https://docs.dex.ag/api/price
    /// Returns the price of one unit of `to` expressed in `from`.
    fn get_price(&self, from: &Token, to: &Token) -> Result<f64>;
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct Token {
    pub name: String,
    pub symbol: String,
    #[serde(default)]
    #[serde(deserialize_with = "deserialize_address")]
    pub address: Option<Address>,
}

#[derive(Clone, Debug, Deserialize)]
struct Price {
    #[serde(with = "display_fromstr")]
    price: f64,
}

fn deserialize_address<'de, D>(deserializer: D) -> Result<Option<Address>, D::Error>
where
    D: Deserializer<'de>,
{
    fn hex_string_to_address(string: &str) -> Result<Address> {
        let prefix = "0x";
        if !string.starts_with(prefix) {
            return Err(anyhow!("does not start with {}", prefix));
        }
        Ok(string[2..].parse()?)
    }

    let string: Option<String> = Option::deserialize(deserializer)?;
    Ok(if let Some(string) = string {
        let address = hex_string_to_address(&string)
            .with_context(|| format!("failed to parse address \"{}\"", string))
            .map_err(D::Error::custom)?;
        Some(address)
    } else {
        None
    })
}

#[derive(Debug)]
pub struct DexagHttpApi {
    base_url: Url,
    client: HttpClient,
}

pub const DEFAULT_BASE_URL: &str = "https://api-v2.dex.ag";
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(10);

impl DexagHttpApi {
    pub fn new() -> Result<Self> {
        Self::custom(DEFAULT_BASE_URL, DEFAULT_TIMEOUT)
    }

    pub fn custom(base_url: &str, timeout: Duration) -> Result<Self> {
        let client = HttpClient::builder()
            .timeout(timeout)
            .build()
            .context("failed to initialize HTTP client")?;
        let base_url = base_url
            .parse()
            .with_context(|| format!("failed to parse url {}", base_url))?;
        Ok(DexagHttpApi { base_url, client })
    }
}

impl DexagApi for DexagHttpApi {
    fn get_token_list(&self) -> Result<Vec<Token>> {
        let mut url = self.base_url.clone();
        url.set_path("token-list-full");
        self.client
            .get(url.to_string())
            .context("failed to retrieve token list from dexag")?
            .json::<Vec<Token>>()
            .context("failed to parse token list json from dexag response")
    }

    fn get_price(&self, from: &Token, to: &Token) -> Result<f64> {
        let mut url = self.base_url.clone();
        url.set_path("price");
        {
            // This is in its own block because we are supposed to drop `q`.
            let mut q = url.query_pairs_mut();
            q.append_pair("from", &from.symbol);
            q.append_pair("to", &to.symbol);
            q.append_pair("fromAmount", "1");
            q.append_pair("dex", "ag");
        }
        self.client
            .get(url.to_string())
            .context("failed to retrieve price from dexag")?
            .json::<Price>()
            .context("failed to parse price json from dexag")
            .map(|price| price.price)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[allow(clippy::float_cmp)]
    fn deserialize_price() {
        let json = r#"{"dex":"ag","price":"220.4","pair":{"base":"ETH","quote":"DAI"},"liquidity":{"uniswap":100}}"#;
        let price: Price = serde_json::from_str(json).unwrap();
        assert_eq!(price.price, 220.4);
    }

    #[test]
    fn deserialize_token() {
        use std::str::FromStr as _;
        let json = r#"{"name":"SAI old DAI (SAI)","symbol":"SAI","address":"0x89d24a6b4ccb1b6faa2625fe562bdd9a23260359"}"#;
        let token: Token = serde_json::from_str(json).unwrap();
        assert_eq!(token.name, "SAI old DAI (SAI)");
        assert_eq!(token.symbol, "SAI");
        assert_eq!(
            token.address.unwrap(),
            Address::from_str("89d24a6b4ccb1b6faa2625fe562bdd9a23260359").unwrap()
        );
    }

    #[test]
    fn deserialize_token_no_address() {
        let json = r#"{"name":"SAI old DAI (SAI)","symbol":"SAI"}"#;
        let token: Token = serde_json::from_str(json).unwrap();
        assert_eq!(token.name, "SAI old DAI (SAI)");
        assert_eq!(token.symbol, "SAI");
        assert!(token.address.is_none());
    }

    // Run with `cargo test online_dexag_api -- --include-ignored --nocapture`.
    #[test]
    #[ignore]
    fn online_dexag_api() {
        let api = DexagHttpApi::new().unwrap();
        let tokens = api.get_token_list().unwrap();
        println!("{:?}", tokens);
        let price = api.get_price(&tokens[0], &tokens[1]).unwrap();
        println!("{:?}", price);
    }
}
