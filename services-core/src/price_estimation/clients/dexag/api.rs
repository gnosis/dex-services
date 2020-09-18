use super::super::generic_client::{Api, GenericToken};
use crate::http::{HttpClient, HttpFactory, HttpLabel};
use anyhow::{Context, Result};
use ethcontract::Address;
use isahc::prelude::Configurable;
use serde::Deserialize;
use serde_with::rust::display_fromstr;
use std::time::Duration;
use url::Url;

#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct Token {
    pub name: String,
    pub symbol: String,
    pub address: Option<Address>,
}
impl GenericToken for Token {
    fn symbol(&self) -> &str {
        &self.symbol
    }
}

#[derive(Clone, Debug, Deserialize)]
struct Price {
    #[serde(with = "display_fromstr")]
    price: f64,
}

#[derive(Debug)]
pub struct DexagHttpApi {
    base_url: Url,
    client: HttpClient,
}
impl DexagHttpApi {
    pub fn with_url(http_factory: &HttpFactory, base_url: &str) -> Result<Self> {
        let client = http_factory
            .with_config(|builder| builder.timeout(Duration::from_secs(30)))
            .context("failed to initialize HTTP client")?;
        let base_url = base_url
            .parse()
            .with_context(|| format!("failed to parse url {}", base_url))?;
        Ok(DexagHttpApi { base_url, client })
    }
}

pub const DEFAULT_BASE_URL: &str = "https://api-v2.dex.ag";

/// Parts of the dex.ag api.
#[async_trait::async_trait]
impl Api for DexagHttpApi {
    type Token = Token;

    fn bind(http_factory: &HttpFactory) -> Result<Self> {
        Self::with_url(http_factory, DEFAULT_BASE_URL)
    }

    /// https://docs.dex.ag/api/tokens
    async fn get_token_list(&self) -> Result<Vec<Token>> {
        let mut url = self.base_url.clone();
        url.set_path("token-list-full");
        self.client
            .get_json_async(url.to_string(), HttpLabel::Dexag)
            .await
            .context("failed to get token list from dexag response")
    }

    /// https://docs.dex.ag/api/price
    async fn get_price(&self, from: &Token, to: &Token) -> Result<f64> {
        let mut url = self.base_url.clone();
        url.set_path("price");
        url.query_pairs_mut()
            .append_pair("from", &from.symbol)
            .append_pair("to", &to.symbol)
            .append_pair("fromAmount", "1")
            .append_pair("dex", "ag");

        Ok(self
            .client
            .get_json_async::<_, Price>(url.as_str(), HttpLabel::Dexag)
            .await
            .context("failed to get price from dexag")?
            .price)
    }

    fn reference_token_symbol() -> &'static str {
        &"DAI"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::util::FutureWaitExt as _;

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

    // Run with `cargo test online_dexag_api -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn online_dexag_api() {
        let api = DexagHttpApi::bind(&HttpFactory::default()).unwrap();
        let tokens = api.get_token_list().wait().unwrap();
        println!("{:#?}", tokens);

        let price = api.get_price(&tokens[0], &tokens[1]).wait().unwrap();
        println!("{:#?}", price);
    }
}
