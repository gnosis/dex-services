use super::{PriceSource, Token};
use crate::models::TokenId;
use anyhow::{anyhow, Result};
use std::collections::HashMap;

/// Combines two prices into one average.
pub struct AveragePriceSource<T0, T1> {
    source_0: T0,
    source_1: T1,
}

impl<T0, T1> AveragePriceSource<T0, T1> {
    pub fn new(source_0: T0, source_1: T1) -> Self {
        Self { source_0, source_1 }
    }
}

impl<T0: PriceSource, T1: PriceSource> PriceSource for AveragePriceSource<T0, T1> {
    fn get_prices(&self, tokens: &[Token]) -> Result<HashMap<TokenId, u128>> {
        match (
            self.source_0.get_prices(tokens),
            self.source_1.get_prices(tokens),
        ) {
            (Ok(p0), Ok(p1)) => Ok(average_prices(p0, p1)),
            (Ok(p), Err(e)) | (Err(e), Ok(p)) => {
                log::warn!("one price source failed: {}", e);
                Ok(p)
            }
            (Err(e0), Err(e1)) => Err(anyhow!("both price sources failed: {}, {}", e0, e1)),
        }
    }
}

fn average_prices(
    prices_0: HashMap<TokenId, u128>,
    mut prices_1: HashMap<TokenId, u128>,
) -> HashMap<TokenId, u128> {
    for (token_id, price_1) in prices_0 {
        prices_1
            .entry(token_id)
            .and_modify(|price| *price = (*price + price_1) / 2)
            .or_insert(price_1);
    }
    prices_1
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn average_prices_() {
        let p0 = hash_map! {
            TokenId(0) => 0,
            TokenId(1) => 5,
            TokenId(2) => 10,
            TokenId(3) => 20,
            TokenId(5) => 0,
        };
        let p1 = hash_map! {
            TokenId(0) => 0,
            TokenId(1) => 5,
            TokenId(2) => 20,
            TokenId(4) => 30,
            TokenId(5) => 100,
        };
        let result = average_prices(p0, p1);
        let expected = hash_map! {
            TokenId(0) => 0,
            TokenId(1) => 5,
            TokenId(2) => 15,
            TokenId(3) => 20,
            TokenId(4) => 30,
            TokenId(5) => 50,
        };
        assert_eq!(result, expected);
    }
}
