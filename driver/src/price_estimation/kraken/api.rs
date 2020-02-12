use anyhow::{anyhow, Context, Result};
use isahc::prelude::*;
use serde::Deserialize;
use std::collections::HashMap;

/// A trait representing a Kraken API client.
///
/// Note that this is not the full API, only the subset required for the
/// retrieving price estimates for the solver.
//#[cfg_attr(test, mockall::automock)]
pub trait KrakenApi {
    /// Retrieves the list of supported assets.
    fn assets(&self) -> Result<HashMap<String, Asset>>;
    /// Retrieves the list of supported asset pairs.
    fn asset_pairs(&self) -> Result<HashMap<String, AssetPair>>;
    /// Retrieves ticker information (with recent prices) for the given asset
    /// pair identifiers.
    fn ticker(&self, pairs: &[&str]) -> Result<HashMap<String, TickerInfo>>;
}

/// An HTTP Kraken API Client.
#[derive(Debug)]
pub struct HttpApi {
    /// The base URL for the API calls.
    base_url: String,
    /// An HTTP client for all of the HTTP requests.
    client: HttpClient,
}

impl KrakenApi for HttpApi {
    fn assets(&self) -> Result<HashMap<String, Asset>> {
        self.client
            .get(format!("{}/Assets", self.base_url))
            .context("failed to retrieve list of assets from Kraken")?
            .json::<KrakenResult<_>>()
            .context("failed to parse assets JSON")?
            .into_result()
    }

    fn asset_pairs(&self) -> Result<HashMap<String, AssetPair>> {
        self.client
            .get(format!("{}/AssetPairs", self.base_url))
            .context("failed to retrieve list of asset pairs from Kraken")?
            .json::<KrakenResult<_>>()
            .context("failed to parse asset pairs JSON")?
            .into_result()
    }

    fn ticker(&self, pairs: &[&str]) -> Result<HashMap<String, TickerInfo>> {
        self.client
            .get(format!("{}/Ticker?pair={}", self.base_url, pairs.join(",")))
            .context("failed to retrieve ticker infos from Kraken")?
            .json::<KrakenResult<_>>()
            .context("failed to parse ticker JSON")?
            .into_result()
    }
}

/// The result type that is returned by Kraken on API requests. This type is
/// only used internally.
#[derive(Clone, Debug, Deserialize)]
struct KrakenResult<T> {
    errors: Vec<String>,
    result: Option<T>,
}

impl<T> KrakenResult<T> {
    fn into_result(self) -> Result<T> {
        if let Some(result) = self.result {
            Ok(result)
        } else if !self.errors.is_empty() {
            Err(anyhow!("Kraken API errors: {:?}", self.errors))
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
struct Asset {
    altname: String,
}

/// A struct representing an asset pair retrieved from the Kraken API.
///
/// Note that this is only a small subset of the data provided by the Kraken API
/// and only the parts required for retrieving price estimates for the solver
/// are included.
#[derive(Clone, Debug, Deserialize)]
struct AssetPair {
    altname: String,
}

/// A struct representing ticker info for an asset pair including price
/// information.
///
/// Note that this is only a small subset of the data provided by the Kraken API
/// and only the parts required for retrieving price estimates for the solver
/// are included.
#[derive(Clone, Debug, Deserialize)]
struct TickerInfo {
    p: PricePair,
}

/// A price pair used in the ticker info, where the first field is today's price
/// and the second field is from the last 24 hours.
#[derive(Clone, Debug, Deserialize)]
struct PricePair(f64, f64);
