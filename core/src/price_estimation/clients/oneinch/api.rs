use super::super::generic_client::{Api, GenericToken};
use crate::http::{HttpClient, HttpFactory, HttpLabel};
use anyhow::{Context, Result};
use ethcontract::Address;
use futures::future::{BoxFuture, FutureExt as _};
use serde::Deserialize;
use serde_with::rust::display_fromstr;
use std::collections::HashMap;
use url::Url;

#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct Token {
    pub name: String,
    pub symbol: String,
    pub address: Option<Address>,
    pub decimals: u8,
}
impl GenericToken for Token {
    fn symbol(&self) -> &str {
        &self.symbol
    }
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct TradedAmounts {
    #[serde(with = "display_fromstr")]
    to_token_amount: u128,
    #[serde(with = "display_fromstr")]
    from_token_amount: u128,
}

#[derive(Debug)]
pub struct OneinchHttpApi {
    api_url: Url,
    client: HttpClient,
}

impl OneinchHttpApi {
    pub fn with_url(http_factory: &HttpFactory, api_url: &str) -> Result<Self> {
        let client = http_factory
            .create()
            .context("failed to initialize HTTP client")?;
        let api_url = api_url
            .parse()
            .with_context(|| format!("failed to parse url {}", api_url))?;
        Ok(OneinchHttpApi { api_url, client })
    }
}

pub const DEFAULT_API_URL: &str = "https://api.1inch.exchange/v1.1/";

/// 1inch API version v1.1
/// https://1inch.exchange/#/api
impl Api for OneinchHttpApi {
    type Token = Token;

    fn bind(http_factory: &HttpFactory) -> Result<Self> {
        Self::with_url(http_factory, DEFAULT_API_URL)
    }

    fn get_token_list<'a>(&'a self) -> BoxFuture<'a, Result<Vec<Token>>> {
        async move {
            let url = self.api_url.join("tokens")?;
            let token_mapping: HashMap<String, Token> = self
                .client
                .get_json_async(url.to_string(), HttpLabel::Oneinch)
                .await
                .context("failed to parse token list json from 1inch response")?;
            Ok(token_mapping
                .into_iter()
                .map(|(_token_symbol, token_data)| token_data)
                .collect())
        }
        .boxed()
    }

    fn get_price<'a>(&'a self, from: &Token, to: &Token) -> BoxFuture<'a, Result<f64>> {
        // 1inch requires the user to specify the amount traded in atoms.
        // We compute the price when selling one full token to avoid unavoidable rounding
        // artifacts when selling exactly one token atom.
        let one_token_from = 10_u128.pow(from.decimals as u32).to_string();

        let url = self.api_url.join("quote").and_then(move |mut url| {
            url.query_pairs_mut()
                .append_pair("fromTokenSymbol", &from.symbol)
                .append_pair("toTokenSymbol", &to.symbol)
                .append_pair("amount", &one_token_from);
            Ok(url)
        });

        let decimal_correction = 10_f64.powi(from.decimals as i32 - to.decimals as i32);

        async move {
            let traded_amounts: TradedAmounts = self
                .client
                .get_json_async::<_, TradedAmounts>(url?.as_str(), HttpLabel::Oneinch)
                .await
                .context("failed to parse price json from 1inch")?;
            let num = traded_amounts.to_token_amount as f64;
            let den = traded_amounts.from_token_amount as f64;
            Ok(decimal_correction * num / den)
        }
        .boxed()
    }

    fn reference_token_symbol() -> &'static str {
        &"OWL"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::util::FutureWaitExt as _;

    #[test]
    #[allow(clippy::float_cmp)]
    fn deserialize_price() {
        let json = r#"{"fromToken":{"symbol":"ETH","name":"Ethereum","decimals":18,"address":"0xEeeeeEeeeEeEeeEeEeEeeEEEeeeeEeeeeeeeEEeE"},"toToken":{"symbol":"DAI","name":"Dai Stablecoin","decimals":18,"address":"0x6b175474e89094c44da98b954eedeac495271d0f"},"toTokenAmount":"23897808590784919590159","fromTokenAmount":"100000000000000000000","exchanges":[{"name":"MultiSplit","part":100},{"name":"Mooniswap","part":0},{"name":"Oasis","part":0},{"name":"Kyber","part":0},{"name":"Uniswap","part":0},{"name":"Balancer","part":0},{"name":"PMM","part":0},{"name":"Uniswap V2","part":0},{"name":"0x Relays","part":0},{"name":"0x API","part":0},{"name":"AirSwap","part":0}]}"#;
        let price: TradedAmounts = serde_json::from_str(json).unwrap();
        assert_eq!(price.to_token_amount, 23897808590784919590159);
        assert_eq!(price.from_token_amount, 100000000000000000000);
    }

    #[test]
    fn deserialize_token() {
        use std::str::FromStr as _;
        let json = r#"{"symbol":"SAI","name":"Sai Stablecoin","decimals":18,"address":"0x89d24a6b4ccb1b6faa2625fe562bdd9a23260359"}"#;
        let token: Token = serde_json::from_str(json).unwrap();
        assert_eq!(token.name, "Sai Stablecoin");
        assert_eq!(token.symbol, "SAI");
        assert_eq!(token.decimals, 18);
        assert_eq!(
            token.address.unwrap(),
            Address::from_str("89d24a6b4ccb1b6faa2625fe562bdd9a23260359").unwrap()
        );
    }

    #[test]
    fn deserialize_token_no_address() {
        let json = r#"{"symbol":"SAI","name":"Sai Stablecoin","decimals":18}"#;
        let token: Token = serde_json::from_str(json).unwrap();
        assert_eq!(token.name, "Sai Stablecoin");
        assert_eq!(token.symbol, "SAI");
        assert_eq!(token.decimals, 18);
        assert!(token.address.is_none());
    }

    // Run with `cargo test online_oneinch_api -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn online_oneinch_api() {
        use crate::metrics::HttpMetrics;
        use std::time::Duration;

        let timeout = 20;
        let api = OneinchHttpApi::bind(&HttpFactory::new(
            Duration::from_secs(timeout),
            HttpMetrics::default(),
        ))
        .unwrap();
        let tokens = api.get_token_list().wait().unwrap();
        println!("{:#?}", tokens);

        let dai_index = tokens.iter().position(|data| data.symbol == "DAI").unwrap();
        let eth_index = tokens.iter().position(|data| data.symbol == "ETH").unwrap();
        let usdc_index = tokens
            .iter()
            .position(|data| data.symbol == "USDC")
            .unwrap();
        let price = api
            .get_price(&tokens[eth_index], &tokens[dai_index])
            .wait()
            .unwrap();
        println!("1 ETH = {:#?} DAI", price);
        let price = api
            .get_price(&tokens[eth_index], &tokens[usdc_index])
            .wait()
            .unwrap();
        println!("1 ETH = {:#?} USDC", price);
    }
}
