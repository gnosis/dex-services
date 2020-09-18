mod api;

use super::generic_client::GenericClient;
use api::DexagHttpApi;

pub type DexagClient = GenericClient<DexagHttpApi>;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        http::HttpFactory,
        models::TokenId,
        price_estimation::price_source::PriceSource,
        token_info::hardcoded::{TokenData, TokenInfoOverride},
        util::FutureWaitExt as _,
    };
    use ethcontract::Address;
    use std::sync::Arc;

    // Run with `cargo test online_dexag_client -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn online_dexag_client() {
        use std::time::Instant;

        let address = Address::from_low_u64_be(0);
        let tokens = hash_map! {
            TokenId(1) => TokenInfoOverride::new(address, "WETH", 18, None),
            TokenId(2) => TokenInfoOverride::new(address, "USDT", 6, None),
            TokenId(3) => TokenInfoOverride::new(address, "TUSD", 18, None),
            TokenId(4) => TokenInfoOverride::new(address, "USDC", 6, None),
            TokenId(5) => TokenInfoOverride::new(address, "PAX", 18, None),
            TokenId(6) => TokenInfoOverride::new(address, "GUSD", 2, None),
            TokenId(7) => TokenInfoOverride::new(address, "DAI", 18, None),
            TokenId(8) => TokenInfoOverride::new(address, "sETH", 18, None),
            TokenId(9) => TokenInfoOverride::new(address, "sUSD", 18, None),
            TokenId(15) => TokenInfoOverride::new(address, "SNX", 18, None)
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
