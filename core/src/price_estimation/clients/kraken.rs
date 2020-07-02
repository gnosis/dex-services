//! Implementation of a price source for Kraken.

mod api;

use self::api::{Asset, AssetPair, KrakenApi, KrakenHttpApi};
use super::super::PriceSource;
use crate::http::HttpFactory;
use crate::models::TokenId;
use crate::token_info::{TokenBaseInfo, TokenInfoFetching};
use anyhow::{anyhow, Context, Result};
use futures::future::{self, BoxFuture, FutureExt as _};
use futures::stream::{self, StreamExt};
use std::collections::HashMap;
use std::sync::Arc;

/// A client to the Kraken exchange.
pub struct KrakenClient<Api> {
    /// A Kraken API implementation. This allows for mocked Kraken APIs to be
    /// used for testing.
    api: Api,
    token_info_fetcher: Arc<dyn TokenInfoFetching>,
}

impl KrakenClient<KrakenHttpApi> {
    /// Creates a new client instance using an HTTP API instance and the default
    /// Kraken API base URL.
    pub fn new(
        http_factory: &HttpFactory,
        token_info_fetcher: Arc<dyn TokenInfoFetching>,
    ) -> Result<Self> {
        let api = KrakenHttpApi::new(http_factory)?;
        Ok(KrakenClient::with_api_and_tokens(api, token_info_fetcher))
    }
}

type TokenIdAndInfo = (TokenId, TokenBaseInfo);

impl<Api> KrakenClient<Api>
where
    Api: KrakenApi,
{
    /// Create a new client instance from an API.
    pub fn with_api_and_tokens(api: Api, token_info_fetcher: Arc<dyn TokenInfoFetching>) -> Self {
        KrakenClient {
            api,
            token_info_fetcher,
        }
    }

    // Clippy complains about this but the lifetimes are needed.
    /// Generates a mapping between Kraken asset pair identifiers and tokens
    /// that are used when computing the price map.
    #[allow(clippy::needless_lifetimes)]
    async fn get_token_asset_pairs(
        &self,
        tokens: &[TokenIdAndInfo],
    ) -> Result<HashMap<String, TokenIdAndInfo>> {
        // TODO(nlordell): If these calls start taking too long, we can consider
        //   caching this information somehow. The only thing that is
        //   complicated is determining when the cache needs to be invalidated
        //   as new assets get added to Kraken.

        let (assets, asset_pairs) =
            future::try_join(self.api.assets(), self.api.asset_pairs()).await?;

        let usd = find_asset("USD", &assets)
            .ok_or_else(|| anyhow!("unable to locate USD asset"))?
            .to_owned();

        let token_assets = tokens
            .iter()
            .filter_map(|(token_id, token_info)| {
                let asset = find_asset(token_info.symbol(), &assets)?;
                let pair = find_asset_pair(asset, &usd, &asset_pairs)?;
                Some((pair.to_owned(), (*token_id, token_info.clone())))
            })
            .collect();

        Ok(token_assets)
    }
}

impl<Api> PriceSource for KrakenClient<Api>
where
    Api: KrakenApi + Sync + Send,
{
    fn get_prices<'a>(
        &'a self,
        tokens: &'a [TokenId],
    ) -> BoxFuture<'a, Result<HashMap<TokenId, u128>>> {
        async move {
            let token_infos: Vec<_> = stream::iter(tokens)
                .filter_map(|token| async move {
                    Some((
                        *token,
                        self.token_info_fetcher.get_token_info(*token).await.ok()?,
                    ))
                })
                .collect()
                .await;

            let token_asset_pairs = self
                .get_token_asset_pairs(&token_infos)
                .await
                .context("failed to generate asset pairs mapping for tokens")?;

            let asset_pairs: Vec<_> = token_asset_pairs.keys().map(String::as_str).collect();
            let ticker_infos = self.api.ticker(&asset_pairs).await?;

            let prices = stream::iter(ticker_infos)
                .filter_map(|(pair, info)| {
                    let token_id_and_pair = token_asset_pairs.get(&pair);
                    async move {
                        let price = token_id_and_pair?.1.get_owl_price(info.p.last_24h());
                        Some((token_id_and_pair?.0, price))
                    }
                })
                .collect()
                .await;

            Ok(prices)
        }
        .boxed()
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
    use crate::token_info::{hardcoded::TokenData, TokenBaseInfo};
    use crate::util::FutureWaitExt as _;
    use std::collections::HashSet;
    use std::time::Instant;

    #[test]
    fn get_token_prices() {
        let tokens = hash_map! {
            TokenId(1) => TokenBaseInfo::new("ETH", 18, 0),
            TokenId(4) => TokenBaseInfo::new("USDC", 6, 0),
            TokenId(5) => TokenBaseInfo::new("PAX", 18, 0),
        };

        let mut api = MockKrakenApi::new();
        api.expect_assets().returning(|| {
            async {
                Ok(hash_map! {
                    "USDC" => Asset::new("USDC"),
                    "XETH" => Asset::new("ETH"),
                    "ZUSD" => Asset::new("USD"),
                })
            }
            .boxed()
        });
        api.expect_asset_pairs().returning(|| {
            async {
                Ok(hash_map! {
                    "USDCUSD" => AssetPair::new("USDC", "ZUSD"),
                    "XETHZUSD" => AssetPair::new("XETH", "ZUSD"),
                })
            }
            .boxed()
        });
        api.expect_ticker()
            .withf(|pairs| {
                let unordered_pairs: HashSet<_> = pairs.iter().collect();
                unordered_pairs == ["USDCUSD", "XETHZUSD"].iter().collect()
            })
            .returning(|_| {
                async {
                    Ok(hash_map! {
                        "USDCUSD" => TickerInfo::new(1.0, 1.01),
                        "XETHZUSD" => TickerInfo::new(100.0, 99.0),
                    })
                }
                .boxed()
            });

        let client = KrakenClient::with_api_and_tokens(api, Arc::new(TokenData::from(tokens)));
        let prices = client
            .get_prices(&[1.into(), 4.into(), 5.into()])
            .now_or_never()
            .unwrap()
            .unwrap();

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

        let tokens = hash_map! {
            TokenId(1) => TokenBaseInfo::new("WETH", 18, 0),
            TokenId(2) => TokenBaseInfo::new("USDT", 6, 0),
            TokenId(3) => TokenBaseInfo::new("TUSD", 18, 0),
            TokenId(4) => TokenBaseInfo::new("USDC", 6, 0),
            TokenId(5) => TokenBaseInfo::new("PAX", 18, 0),
            TokenId(6) => TokenBaseInfo::new("GUSD", 2, 0),
            TokenId(7) => TokenBaseInfo::new("DAI", 18, 0),
            TokenId(8) => TokenBaseInfo::new("sETH", 18, 0),
            TokenId(9) => TokenBaseInfo::new("sUSD", 18, 0),
            TokenId(15) => TokenBaseInfo::new("SNX", 18, 0)
        };
        let token_ids: Vec<TokenId> = tokens.keys().copied().collect();

        let start_time = Instant::now();
        {
            let client =
                KrakenClient::new(&HttpFactory::default(), Arc::new(TokenData::from(tokens)))
                    .unwrap();
            let prices = client.get_prices(&token_ids).wait().unwrap();

            println!("{:#?}", prices);
            assert!(
                prices.contains_key(&TokenId(1)),
                "expected ETH price to be found"
            );
        }
        let elapsed_millis = start_time.elapsed().as_secs_f64() * 1000.0;
        println!("Total elapsed time: {}ms", elapsed_millis);
    }
}
