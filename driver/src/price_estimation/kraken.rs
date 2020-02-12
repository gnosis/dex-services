//! Implementation of a price source for Kraken.

mod api;

use self::api::KrakenApi;
use crate::price_estimation::{PriceSource, Token, TokenId};
use anyhow::{Context, Result};
use std::collections::HashMap;

/// A client to the Kraken exchange.
pub struct KrakenClient {
    /// A Kraken API implementation. This allows for mocked Kraken APIs to be
    /// used for testing.
    api: Box<dyn KrakenApi>,
}

const DEFAULT_API_BASE_URL: &str = "https://api.kraken.com/0/public";

#[allow(dead_code)]
impl KrakenClient {
    pub fn new() -> Result<Self> {
        KrakenClient::with_url(DEFAULT_API_BASE_URL)
    }

    pub fn with_url(base_url: &str) -> Result<Self> {
        todo!();
    }

    pub fn with_api(api: impl KrakenApi + 'static) -> Self {
        KrakenClient {
            api: Box::new(api),
            asset_pairs: HashMap::new(),
        }
    }

    fn asset_pairs(&self, tokens: &[Token]) -> Result<HashMap<TokenId, String>> {
        // TODO(nlordell): If these calls start taking too long, we can consider
        //   caching this information somehow. The only thing that is
        //   complicated is determining when the cache needs to be invalidated
        //   as new assets get added to Kraken.

        let assets = self.api.assets()?;
        let asset_pairs = self.api.asset_pairs()?;

        Ok(HashMap::new())
    }
}

impl PriceSource for KrakenClient {
    fn get_prices(&mut self, tokens: &[Token]) -> Result<HashMap<TokenId, u128>> {
        self.update_asset_pairs(tokens)?;
        todo!()
    }
}
