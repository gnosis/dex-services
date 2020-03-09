use super::{PriceSource, Token};
use crate::models::TokenId;
use anyhow::Result;
use std::collections::HashMap;
use std::time::Instant;

/// Implements `PriceSource` in a non blocking way by keeping an internal
/// collection of tracked tokens and their price and last update. This
/// collection has to be updated manually. It does not query an external price
/// source on its own.
pub struct ManuallyUpdatedPriceSource {
    price_map: HashMap<TokenId, (Token, Price)>,
}

impl ManuallyUpdatedPriceSource {
    pub fn new() -> Self {
        ManuallyUpdatedPriceSource {
            price_map: HashMap::new(),
        }
    }

    /// Does nothing if the token is already being tracked.
    pub fn track_tokens(&mut self, tokens: &[Token]) {
        for token in tokens {
            self.price_map
                .entry(token.id)
                .or_insert_with(|| (token.clone(), Price::never_updated()));
        }
    }

    /// Get each token whose price has never been retrieved or whose last price
    /// update happened before the cutoff.
    pub fn tokens_that_need_updating(&self, cutoff: Instant) -> Vec<Token> {
        self.price_map
            .values()
            .filter(|(_token, price)| price.last_update_older_than(cutoff))
            .map(|(token, _price)| token.clone())
            .collect()
    }

    /// For each token:
    /// * Track it if it is not already being tracked.
    /// * Set its last update time to `update_time`.
    /// * If the price is available in `new_prices` then update it.
    pub fn update_tokens(
        &mut self,
        tokens: &[Token],
        new_prices: &HashMap<TokenId, u128>,
        update_time: Instant,
    ) {
        for token in tokens {
            let (_token, price) = self
                .price_map
                .entry(token.id)
                .or_insert_with(|| (token.clone(), Price::never_updated()));
            if let Some(new_price) = new_prices.get(&token.id) {
                price.price.replace(*new_price);
            }
            price.last_update = Some(update_time);
        }
    }
}

impl PriceSource for ManuallyUpdatedPriceSource {
    /// Non blocking.
    /// Infallible.
    fn get_prices(&self, tokens: &[Token]) -> Result<HashMap<TokenId, u128>> {
        Ok(tokens
            .iter()
            .flat_map(|token| {
                let (_token, price) = self.price_map.get(&token.id)?;
                let price = price.price?;
                Some((token.id, price))
            })
            .collect())
    }
}

#[derive(Clone, Debug)]
struct Price {
    price: Option<u128>,
    last_update: Option<Instant>,
}

impl Price {
    fn never_updated() -> Self {
        Self {
            price: None,
            last_update: None,
        }
    }

    fn last_update_older_than(&self, cutoff: Instant) -> bool {
        self.last_update
            .map(|instant| instant < cutoff)
            .unwrap_or(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    lazy_static::lazy_static! {
        static ref TOKENS: [Token; 3] = [
            Token::new(0, "0", 0),
            Token::new(1, "1", 1),
            Token::new(2, "2", 2),
        ];
    }

    fn sorted(mut tokens: Vec<Token>) -> Vec<Token> {
        tokens.sort_by_key(|token| token.id);
        tokens
    }

    #[test]
    fn track_tokens_new_token() {
        let mut nbps = ManuallyUpdatedPriceSource::new();
        let tokens = &TOKENS[0..1];
        nbps.track_tokens(tokens);
        assert_eq!(nbps.tokens_that_need_updating(Instant::now()), tokens);
    }

    #[test]
    fn track_tokens_existing_token() {
        let mut nbps = ManuallyUpdatedPriceSource::new();
        let tokens = &TOKENS[0..1];
        let prices = hash_map! {TOKENS[0].id => 1};
        let instant = Instant::now();

        nbps.update_tokens(tokens, &prices, instant);
        assert!(nbps.tokens_that_need_updating(instant).is_empty());

        nbps.track_tokens(tokens);
        assert!(nbps.tokens_that_need_updating(instant).is_empty());
    }

    #[test]
    fn tokens_that_need_updating() {
        let mut nbps = ManuallyUpdatedPriceSource::new();
        let tokens = &TOKENS[0..2];
        let instant = Instant::now();
        let instant_next = instant + Duration::from_secs(1);
        let prices = hash_map! {TOKENS[0].id => 1};

        nbps.update_tokens(tokens, &prices, instant);
        assert!(nbps.tokens_that_need_updating(instant).is_empty());
        nbps.update_tokens(&TOKENS[0..2], &prices, instant);
        assert_eq!(sorted(nbps.tokens_that_need_updating(instant_next)), tokens);
        nbps.update_tokens(&TOKENS[1..2], &hash_map! {}, instant_next);
        assert_eq!(nbps.tokens_that_need_updating(instant_next), &TOKENS[0..1]);
    }

    #[test]
    fn update_tokens() {
        let mut nbps = ManuallyUpdatedPriceSource::new();
        let tokens = &TOKENS[0..1];
        let instant = Instant::now();
        let prices = hash_map! {TOKENS[0].id => 1};

        assert!(nbps.get_prices(tokens).unwrap().is_empty());
        nbps.update_tokens(tokens, &prices, instant);
        assert_eq!(nbps.get_prices(tokens).unwrap(), prices);
    }
}
