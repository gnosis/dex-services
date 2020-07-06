mod api;

use super::generic_client::GenericClient;
use api::DexagHttpApi;

pub type DexagClient = GenericClient<DexagHttpApi>;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http::HttpFactory;
    use crate::models::TokenId;
    use crate::price_estimation::price_source::PriceSource;
    use crate::token_info::hardcoded::{TokenData, TokenInfoOverride};
    use crate::util::FutureWaitExt as _;
    use std::sync::Arc;

    // Run with `cargo test online_dexag_client -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn online_dexag_client() {
        use std::time::Instant;

        let tokens = hash_map! {
            TokenId(1) => TokenInfoOverride::new("WETH", 18, 0),
            TokenId(2) => TokenInfoOverride::new("USDT", 6, 0),
            TokenId(3) => TokenInfoOverride::new("TUSD", 18, 0),
            TokenId(4) => TokenInfoOverride::new("USDC", 6, 0),
            TokenId(5) => TokenInfoOverride::new("PAX", 18, 0),
            TokenId(6) => TokenInfoOverride::new("GUSD", 2, 0),
            TokenId(7) => TokenInfoOverride::new("DAI", 18, 0),
            TokenId(8) => TokenInfoOverride::new("sETH", 18, 0),
            TokenId(9) => TokenInfoOverride::new("sUSD", 18, 0),
            TokenId(15) => TokenInfoOverride::new("SNX", 18, 0)
        };
        let mut ids: Vec<TokenId> = tokens.keys().copied().collect();

        let client = DexagClient::new(
            &HttpFactory::default(),
            Arc::new(TokenData::from(tokens.clone())),
        )
        .unwrap();
        let before = Instant::now();
        let prices = client.get_prices(&ids).wait().unwrap();
        let after = Instant::now();
        println!(
            "Took {} seconds to get prices.",
            (after - before).as_secs_f64()
        );

        ids.sort();
        for id in ids {
            let symbol = &tokens.get(&id).unwrap().alias;
            if let Some(price) = prices.get(&id) {
                println!("Token {} has OWL price of {}.", symbol, price);
            } else {
                println!("Token {} price could not be determined.", symbol);
            }
        }
    }
}
