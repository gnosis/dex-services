mod api;

use super::generic_client::GenericClient;
use api::OneinchHttpApi;

pub type OneinchClient = GenericClient<OneinchHttpApi>;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http::HttpFactory;
    use crate::models::TokenId;
    use crate::price_estimation::price_source::PriceSource;
    use crate::token_info::{hardcoded::TokenData, TokenBaseInfo};
    use crate::util::FutureWaitExt as _;
    use std::sync::Arc;

    // Run with `cargo test online_oneinch_client -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn online_oneinch_client() {
        use std::time::Instant;

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
        let mut ids: Vec<TokenId> = tokens.keys().copied().collect();

        let client = OneinchClient::new(
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
            let symbol = tokens.get(&id).unwrap().symbol();
            if let Some(price) = prices.get(&id) {
                println!("Token {} has OWL price of {}.", symbol, price);
            } else {
                println!("Token {} price could not be determined.", symbol);
            }
        }
    }
}
