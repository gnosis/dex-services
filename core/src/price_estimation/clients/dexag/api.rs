use super::super::generic_client::{Api, Symbolic};
use crate::http::{HttpClient, HttpFactory, HttpLabel};
use anyhow::{Context, Result};
use ethcontract::Address;
use futures::future::{BoxFuture, FutureExt as _};
use serde::Deserialize;
use serde_with::rust::display_fromstr;
use url::Url;

#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct Token {
    pub name: String,
    pub symbol: String,
    pub address: Option<Address>,
}
impl Symbolic for Token {
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
            .create()
            .context("failed to initialize HTTP client")?;
        let base_url = base_url
            .parse()
            .with_context(|| format!("failed to parse url {}", base_url))?;
        Ok(DexagHttpApi { base_url, client })
    }
}

pub const DEFAULT_BASE_URL: &str = "https://api-v2.dex.ag";

/// Parts of the dex.ag api.
impl Api for DexagHttpApi {
    type Token = Token;

    fn bind(http_factory: &HttpFactory) -> Result<Self> {
        Self::with_url(http_factory, DEFAULT_BASE_URL)
    }

    /// https://docs.dex.ag/api/tokens
    fn get_token_list<'a>(&'a self) -> BoxFuture<'a, Result<Vec<Token>>> {
        async move {
            let mut url = self.base_url.clone();
            url.set_path("token-list-full");
            self.client
                .get_json_async(url.to_string(), HttpLabel::Dexag)
                .await
                .context("failed to parse token list json from dexag response")
        }
        .boxed()
    }

    /// https://docs.dex.ag/api/price
    fn get_price<'a>(&'a self, from: &Token, to: &Token) -> BoxFuture<'a, Result<f64>> {
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

        async move {
            Ok(self
                .client
                .get_json_async::<_, Price>(url.as_str(), HttpLabel::Dexag)
                .await
                .context("failed to parse price json from dexag")?
                .price)
        }
        .boxed()
    }

    fn stable_coin_symbol() -> String {
        "DAI".to_string()
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
