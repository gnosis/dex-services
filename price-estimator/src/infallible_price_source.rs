use core::{models::TokenId, token_info::TokenBaseInfo};
use std::{collections::HashMap, num::NonZeroU128};

/// Roughly like `PriceSource` but is updated externally and cannot fail.
#[derive(Debug, Default)]
pub struct InfalliblePriceSource {
    token_infos: HashMap<TokenId, TokenBaseInfo>,
    prices: HashMap<TokenId, NonZeroU128>,
}

impl InfalliblePriceSource {
    pub fn new(token_infos: HashMap<TokenId, TokenBaseInfo>) -> Self {
        Self {
            token_infos,
            prices: HashMap::new(),
        }
    }

    pub fn update(&mut self, prices: &HashMap<TokenId, NonZeroU128>) {
        self.prices.extend(prices.iter());
    }

    /// Tries to use the current price from the first source, if that fails one base unit of the
    /// token is used (based on decimals) and if that also fails the fee price is used. If we do not
    /// not have the fee price 10e18 is used.
    pub fn price(&self, token_id: TokenId) -> NonZeroU128 {
        match self.prices.get(&token_id) {
            Some(price) => *price,
            None => match self.token_infos.get(&token_id) {
                Some(token_info) => token_info.base_unit_in_atoms(),
                None => self
                    .prices
                    .get(&TokenId(0))
                    .copied()
                    .unwrap_or_else(|| NonZeroU128::new(10u128.pow(18)).unwrap()),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn use_existing_price() {
        let token = TokenId(1);
        let price = NonZeroU128::new(1).unwrap();
        let mut ips = InfalliblePriceSource::default();
        ips.update(&[(token, price)].iter().copied().collect());
        assert_eq!(ips.price(token), price);
    }

    #[test]
    fn fallback_to_base_unit() {
        let token = TokenId(1);
        let token_info = TokenBaseInfo {
            alias: String::new(),
            decimals: 1,
        };
        let ips = InfalliblePriceSource::new([(token, token_info)].iter().cloned().collect());
        assert_eq!(ips.price(token).get(), 10);
    }

    #[test]
    fn fallback_to_fee() {
        let token = TokenId(1);
        let mut ips = InfalliblePriceSource::default();
        ips.update(
            &[(TokenId(0), NonZeroU128::new(1).unwrap())]
                .iter()
                .copied()
                .collect(),
        );
        assert_eq!(ips.price(token).get(), 1);
    }
}
