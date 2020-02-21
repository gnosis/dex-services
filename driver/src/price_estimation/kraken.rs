//! Implementation of a price source for Kraken.

mod api;

use self::api::{Asset, AssetPair, KrakenApi, KrakenHttpApi};
use super::{PriceSource, Token, TokenId};
use anyhow::{anyhow, Context, Result};
use std::collections::HashMap;

/// A client to the Kraken exchange.
pub struct KrakenClient<Api> {
    /// A Kraken API implementation. This allows for mocked Kraken APIs to be
    /// used for testing.
    api: Api,
}

impl KrakenClient<KrakenHttpApi> {
    /// Creates a new client instance using an HTTP API instance and the default
    /// Kraken API base URL.
    pub fn new() -> Result<Self> {
        let api = KrakenHttpApi::new()?;
        Ok(KrakenClient::with_api(api))
    }
}

impl<Api> KrakenClient<Api>
where
    Api: KrakenApi,
{
    /// Create a new client instance from an API.
    pub fn with_api(api: Api) -> Self {
        KrakenClient { api }
    }

    /// Generates a mapping between Kraken asset pair identifiers and tokens
    /// that are used when computing the price map.
    fn get_token_asset_pairs<'a>(&self, tokens: &'a [Token]) -> Result<HashMap<String, &'a Token>> {
        // TODO(nlordell): If these calls start taking too long, we can consider
        //   caching this information somehow. The only thing that is
        //   complicated is determining when the cache needs to be invalidated
        //   as new assets get added to Kraken.

        let assets = self.api.assets()?;
        let asset_pairs = self.api.asset_pairs()?;

        let usd =
            find_asset("USD", &assets).ok_or_else(|| anyhow!("unable to locate USD asset"))?;

        let token_assets = tokens
            .iter()
            .flat_map(|token| {
                let asset = find_asset(&token.symbol, &assets)?;
                let pair = find_asset_pair(asset, usd, &asset_pairs)?;
                Some((pair.to_owned(), token))
            })
            .collect();

        Ok(token_assets)
    }
}

impl<Api> PriceSource for KrakenClient<Api>
where
    Api: KrakenApi,
{
    fn get_prices(&self, tokens: &[Token]) -> Result<HashMap<TokenId, u128>> {
        let token_asset_pairs = self
            .get_token_asset_pairs(tokens)
            .context("failed to generate asset pairs mapping for tokens")?;

        let asset_pairs: Vec<_> = token_asset_pairs.keys().map(String::as_str).collect();
        let ticker_infos = self.api.ticker(&asset_pairs)?;

        let prices = ticker_infos
            .iter()
            .flat_map(|(pair, info)| {
                let token = token_asset_pairs.get(pair)?;
                let price = token.get_owl_price(info.p.last_24h());

                Some((token.id, price))
            })
            .collect();

        Ok(prices)
    }
}

/// Finds the Kraken asset identifier given a token symbol.
fn find_asset<'a>(symbol: &'a str, assets: &'a HashMap<String, Asset>) -> Option<&'a str> {
    if assets.contains_key(symbol) {
        Some(symbol)
    } else if let Some((asset_name, _)) = assets.iter().find(|(_, asset)| asset.altname == symbol) {
        Some(asset_name)
    } else {
        None
    }
}

/// Finds an asset pair from two Kraken asset identifiers.
fn find_asset_pair<'a>(
    asset: &str,
    to: &str,
    asset_pairs: &'a HashMap<String, AssetPair>,
) -> Option<&'a str> {
    let (pair_name, _) = asset_pairs
        .iter()
        // NOTE: Filter out pairs ending in ".d" as they dont' seem to work for
        //   retrieving ticker info.
        .filter(|&(name, _)| !name.ends_with(".d"))
        .find(|&(_, pair)| pair.base == asset && pair.quote == to)?;

    Some(pair_name)
}

#[cfg(test)]
mod tests {
    use super::api::{MockKrakenApi, TickerInfo};
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn get_token_prices() {
        let tokens = vec![
            Token::test(1, "ETH", 18),
            Token::test(4, "USDC", 6),
            Token::test(5, "PAX", 18),
        ];

        let mut api = MockKrakenApi::new();
        api.expect_assets().returning(|| {
            Ok(hash_map! {
                "USDC" => Asset::new("USDC"),
                "XETH" => Asset::new("ETH"),
                "ZUSD" => Asset::new("USD"),
            })
        });
        api.expect_asset_pairs().returning(|| {
            Ok(hash_map! {
                "USDCUSD" => AssetPair::new("USDC", "ZUSD"),
                "XETHZUSD" => AssetPair::new("XETH", "ZUSD"),
            })
        });
        api.expect_ticker()
            .withf(|pairs| {
                let unordered_pairs: HashSet<_> = pairs.iter().collect();
                unordered_pairs == ["USDCUSD", "XETHZUSD"].iter().collect()
            })
            .returning(|_| {
                Ok(hash_map! {
                    "USDCUSD" => TickerInfo::new(1.0, 1.01),
                    "XETHZUSD" => TickerInfo::new(100.0, 99.0),
                })
            });

        let client = KrakenClient::with_api(api);
        let prices = client.get_prices(&tokens).unwrap();

        assert_eq!(
            prices,
            hash_map! {
                TokenId(1) => (99.0 * 10f64.powi(18)) as u128,
                TokenId(4) => (1.01 * 10f64.powi(30)) as u128,
            }
        );
    }

    #[test]
    #[ignore]
    fn online_kraken_prices() {
        // Retrieve real token prices from Kraken, this test is ignored by
        // default as there is no way to guarantee the service can be connected
        // to and the values are unpredictable. To run this test and output the
        // retrieved price estimates:
        // ```
        // cargo test online_kraken_prices -- --ignored --nocapture
        // ```

        let tokens = vec![
            Token::test(1, "ETH", 18),
            Token::test(2, "USDT", 6),
            Token::test(3, "TUSD", 18),
            Token::test(4, "USDC", 6),
            Token::test(5, "PAX", 18),
            Token::test(6, "GUSD", 2),
            Token::test(7, "DAI", 18),
            Token::test(8, "sETH", 18),
            Token::test(9, "sUSD", 18),
            Token::test(15, "SNX", 18),
        ];

        let client = KrakenClient::new().unwrap();
        let prices = client.get_prices(&tokens).unwrap();

        println!("{:#?}", prices);
        assert!(
            prices.contains_key(&TokenId(1)),
            "expected ETH price to be found"
        );
    }
}
