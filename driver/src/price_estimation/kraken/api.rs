use crate::http::{HttpClient, HttpFactory, HttpLabel};
use anyhow::{anyhow, Context, Result};
use futures::future::{BoxFuture, FutureExt as _};
use serde::Deserialize;
use serde_with::rust::display_fromstr;
use std::collections::HashMap;

/// A trait representing a Kraken API client.
///
/// Note that this is not the full API, only the subset required for the
/// retrieving price estimates for the solver.
#[cfg_attr(test, mockall::automock)]
pub trait KrakenApi {
    /// Retrieves the list of supported assets.
    fn assets<'a>(&'a self) -> BoxFuture<'a, Result<HashMap<String, Asset>>>;
    /// Retrieves the list of supported asset pairs.
    fn asset_pairs<'a>(&'a self) -> BoxFuture<'a, Result<HashMap<String, AssetPair>>>;
    /// Retrieves ticker information (with recent prices) for the given asset
    /// pair identifiers.
    fn ticker<'a, 'b>(
        &'a self,
        pairs: &'b [&'b str],
    ) -> BoxFuture<'a, Result<HashMap<String, TickerInfo>>>;
}

/// An HTTP Kraken API Client.
#[derive(Debug)]
pub struct KrakenHttpApi {
    /// The base URL for the API calls.
    base_url: String,
    /// An HTTP client for all of the HTTP requests.
    client: HttpClient,
}

/// The default Kraken API base URL.
pub const DEFAULT_API_BASE_URL: &str = "https://api.kraken.com/0/public";

impl KrakenHttpApi {
    pub fn new(http_factory: &HttpFactory) -> Result<Self> {
        KrakenHttpApi::with_url(http_factory, DEFAULT_API_BASE_URL)
    }

    pub fn with_url(http_factory: &HttpFactory, base_url: &str) -> Result<Self> {
        let client = http_factory.create()?;
        Ok(KrakenHttpApi {
            base_url: base_url.into(),
            client,
        })
    }
}

impl KrakenApi for KrakenHttpApi {
    fn assets<'a>(&'a self) -> BoxFuture<'a, Result<HashMap<String, Asset>>> {
        async move {
            self.client
                .get_json_async::<_, KrakenResult<_>>(
                    format!("{}/Assets", self.base_url),
                    HttpLabel::Kraken,
                )
                .await
                .context("failed to parse assets JSON")?
                .into_result()
        }
        .boxed()
    }

    fn asset_pairs<'a>(&'a self) -> BoxFuture<'a, Result<HashMap<String, AssetPair>>> {
        async move {
            self.client
                .get_json_async::<_, KrakenResult<_>>(
                    format!("{}/AssetPairs", self.base_url),
                    HttpLabel::Kraken,
                )
                .await
                .context("failed to parse asset pairs JSON")?
                .into_result()
        }
        .boxed()
    }

    fn ticker<'a, 'b>(
        &'a self,
        pairs: &'b [&'b str],
    ) -> BoxFuture<'a, Result<HashMap<String, TickerInfo>>> {
        let url = if pairs.is_empty() {
            None
        } else {
            Some(format!("{}/Ticker?pair={}", self.base_url, pairs.join(",")))
        };
        async move {
            match url {
                None => Ok(HashMap::new()),
                Some(url) => self
                    .client
                    .get_json_async::<_, KrakenResult<_>>(url, HttpLabel::Kraken)
                    .await
                    .context("failed to parse ticker JSON")?
                    .into_result(),
            }
        }
        .boxed()
    }
}

/// The result type that is returned by Kraken on API requests. This type is
/// only used internally.
#[derive(Clone, Debug, Deserialize)]
struct KrakenResult<T> {
    error: Vec<String>,
    result: Option<T>,
}

impl<T> KrakenResult<T> {
    fn into_result(self) -> Result<T> {
        if let Some(result) = self.result {
            Ok(result)
        } else if !self.error.is_empty() {
            Err(anyhow!("Kraken API errors: {:?}", self.error))
        } else {
            Err(anyhow!("unknown Kraken API error"))
        }
    }
}

/// A struct representing an asset retrieved from the Kraken API.
///
/// Note that this is only a small subset of the data provided by the Kraken API
/// and only the parts required for retrieving price estimates for the solver
/// are included.
#[derive(Clone, Debug, Deserialize)]
#[cfg_attr(test, derive(PartialEq))]
pub struct Asset {
    pub altname: String,
}

impl Asset {
    /// Create a new asset from an alternate name.
    #[cfg(test)]
    pub fn new(altname: &str) -> Asset {
        Asset {
            altname: altname.into(),
        }
    }
}

/// A struct representing an asset pair retrieved from the Kraken API.
///
/// Note that this is only a small subset of the data provided by the Kraken API
/// and only the parts required for retrieving price estimates for the solver
/// are included.
#[derive(Clone, Debug, Deserialize)]
#[cfg_attr(test, derive(PartialEq))]
pub struct AssetPair {
    pub base: String,
    pub quote: String,
}

impl AssetPair {
    /// Create a new asset pair from base and quote assets.
    #[cfg(test)]
    pub fn new(base: &str, quote: &str) -> AssetPair {
        AssetPair {
            base: base.into(),
            quote: quote.into(),
        }
    }
}

/// A struct representing ticker info for an asset pair including price
/// information.
///
/// Note that this is only a small subset of the data provided by the Kraken API
/// and only the parts required for retrieving price estimates for the solver
/// are included.
#[derive(Clone, Debug, Deserialize)]
#[cfg_attr(test, derive(PartialEq))]
pub struct TickerInfo {
    pub p: PricePair,
}

impl TickerInfo {
    /// Create a new ticker info from its price pair.
    #[cfg(test)]
    pub fn new(today: f64, last_24h: f64) -> TickerInfo {
        TickerInfo {
            p: PricePair(today, last_24h),
        }
    }
}

/// A price pair used in the ticker info, where the first field is today's price
/// and the second field is from the last 24 hours.
#[derive(Copy, Clone, Debug, Deserialize)]
#[cfg_attr(test, derive(PartialEq))]
pub struct PricePair(
    #[serde(with = "display_fromstr")] f64,
    #[serde(with = "display_fromstr")] f64,
);

impl PricePair {
    /// Retrieves the price for the last 24 hours.
    pub fn last_24h(self) -> f64 {
        self.1
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::util::FutureWaitExt as _;
    use serde::de::DeserializeOwned;

    fn deserialize<T: DeserializeOwned>(json: &str) -> T {
        serde_json::from_str::<KrakenResult<_>>(json)
            .unwrap()
            .into_result()
            .unwrap()
    }

    #[test]
    fn parse_assets_json() {
        // Sample retrieved from https://api.kraken.com/0/public/Assets?asset=DAI,ETH
        let value: HashMap<String, Asset> = deserialize(
            r#"{"error":[],"result":{"DAI":{"aclass":"currency","altname":"DAI","decimals":10,"display_decimals":5},"XETH":{"aclass":"currency","altname":"ETH","decimals":10,"display_decimals":5}}}"#,
        );
        assert_eq!(
            value,
            hash_map! {
                "DAI" => Asset::new("DAI"),
                "XETH" => Asset::new("ETH"),
            }
        );
    }

    #[test]
    fn parse_asset_pairs_json() {
        // Sample retrieved from https://api.kraken.com/0/public/AssetPairs?pair=DAIUSD,ETHUSD
        let value: HashMap<String, AssetPair> = deserialize(
            r#"{"error":[],"result":{"DAIUSD":{"altname":"DAIUSD","wsname":"DAI\/USD","aclass_base":"currency","base":"DAI","aclass_quote":"currency","quote":"ZUSD","lot":"unit","pair_decimals":5,"lot_decimals":8,"lot_multiplier":1,"leverage_buy":[],"leverage_sell":[],"fees":[[0,0.2],[50000,0.16],[100000,0.12],[250000,0.08],[500000,0.04],[1000000,0]],"fees_maker":[[0,0.2],[50000,0.16],[100000,0.12],[250000,0.08],[500000,0.04],[1000000,0]],"fee_volume_currency":"ZUSD","margin_call":80,"margin_stop":40},"XETHZUSD":{"altname":"ETHUSD","wsname":"ETH\/USD","aclass_base":"currency","base":"XETH","aclass_quote":"currency","quote":"ZUSD","lot":"unit","pair_decimals":2,"lot_decimals":8,"lot_multiplier":1,"leverage_buy":[2,3,4,5],"leverage_sell":[2,3,4,5],"fees":[[0,0.26],[50000,0.24],[100000,0.22],[250000,0.2],[500000,0.18],[1000000,0.16],[2500000,0.14],[5000000,0.12],[10000000,0.1]],"fees_maker":[[0,0.16],[50000,0.14],[100000,0.12],[250000,0.1],[500000,0.08],[1000000,0.06],[2500000,0.04],[5000000,0.02],[10000000,0]],"fee_volume_currency":"ZUSD","margin_call":80,"margin_stop":40}}}"#,
        );
        assert_eq!(
            value,
            hash_map! {
                "DAIUSD" => AssetPair::new("DAI", "ZUSD"),
                "XETHZUSD" => AssetPair::new("XETH", "ZUSD"),
            }
        );
    }

    #[test]
    fn parse_ticker_infos_json() {
        // Sample retrieved from https://api.kraken.com/0/public/Ticker?pair=DAIUSD,ETHUSD
        let value: HashMap<String, TickerInfo> = deserialize(
            r#"{"error":[],"result":{"DAIUSD":{"a":["1.00105000","3091","3091.000"],"b":["0.99989000","628","628.000"],"c":["0.99986000","115.05510869"],"v":["267638.81097379","587455.25359912"],"p":["0.99938164","0.99862453"],"t":[1219,2709],"l":["0.99696000","0.99608000"],"h":["1.00300000","1.00300000"],"o":"0.99828000"},"XETHZUSD":{"a":["257.73000","124","124.000"],"b":["257.72000","1","1.000"],"c":["257.81000","6.60000000"],"v":["99246.07873952","130832.28052933"],"p":["251.77023","248.47638"],"t":[13177,18557],"l":["238.06000","230.57000"],"h":["259.48000","259.48000"],"o":"238.06000"}}}"#,
        );
        assert_eq!(
            value,
            hash_map! {
                "DAIUSD" => TickerInfo::new(0.999_381_64f64, 0.998_624_53f64),
                "XETHZUSD" => TickerInfo::new(251.77023, 248.47638),
            }
        );
    }

    #[test]
    #[ignore]
    fn online_kraken_api() {
        // Interact with the online Kraken API to find some assets and get their
        // current prices.
        //
        // This test is ignored by default as there is no way to guarantee the
        // service can be connected to and the values are unpredictable. To run
        // this test and log some output run:
        // ```
        // cargo test online_kraken_api -- --ignored --nocapture
        // ```

        let api = KrakenHttpApi::new(&HttpFactory::default()).unwrap();

        let assets = api.assets().wait().unwrap();
        println!("GNO asset information: {:?}", assets["GNO"]);

        let pairs = api.asset_pairs().wait().unwrap();
        println!("GNO/EUR asset pair: {:?}", pairs["GNOEUR"]);

        let ticker = api.ticker(&["GNOEUR"]).wait().unwrap();
        println!("GNO/EUR ticker information: {:?}", ticker["GNOEUR"]);
    }
}
