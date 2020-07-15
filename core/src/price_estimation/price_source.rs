use crate::models::{TokenId, TokenInfo};
use anyhow::Result;
use futures::future::{BoxFuture, FutureExt as _};
use std::collections::HashMap;

/// A token representation.
#[cfg_attr(test, derive(Eq, PartialEq))]
#[derive(Clone, Debug)]
pub struct Token {
    /// The ID of the token.
    pub id: TokenId,
    /// The token info for this token including, token symbol and number of
    /// decimals.
    pub info: TokenInfo,
}

/// An abstraction around a type that retrieves price estimate from a source
/// such as an exchange.
pub trait PriceSource {
    /// Retrieve current prices relative to the OWL token for the specified
    /// tokens (price denominated in OWL). The OWL token is pegged at 1 USD
    /// with 18 decimals. Returns a sparse price array as being unable to
    /// find a price is not considered an error.
    fn get_prices<'a>(
        &'a self,
        tokens: &'a [TokenId],
    ) -> BoxFuture<'a, Result<HashMap<TokenId, u128>>>;
}

/// A no-op price source that always succeeds and finds no prices.
pub struct NoopPriceSource;

impl PriceSource for NoopPriceSource {
    fn get_prices(&self, _: &[TokenId]) -> BoxFuture<Result<HashMap<TokenId, u128>>> {
        async { Ok(HashMap::new()) }.boxed()
    }
}

/// Wait for the price source to have a price for this token.
/// Useful for ThreadedPriceSource to ensure that it has updated at least once.
pub async fn wait_for_price(price_source: &dyn PriceSource, token_id: TokenId) {
    loop {
        let has_price = match price_source.get_prices(&[token_id]).await {
            Ok(prices) => prices.get(&token_id).is_some(),
            Err(_) => false,
        };
        if has_price {
            return;
        }
        async_std::task::sleep(std::time::Duration::from_secs(1)).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wait_for_price_returns() {
        let mut price_source = MockPriceSource::new();
        price_source
            .expect_get_prices()
            .returning(|_| immediate!(Ok(hash_map!(TokenId(0) => 0))));
        assert!(wait_for_price(&price_source, TokenId(0))
            .now_or_never()
            .is_some());
    }

    #[test]
    fn wait_for_price_waits() {
        let mut price_source = MockPriceSource::new();
        price_source
            .expect_get_prices()
            .returning(|_| futures::future::pending().boxed());
        assert!(wait_for_price(&price_source, TokenId(0))
            .now_or_never()
            .is_none());
    }
}

// We would like to tag `PriceSource` with `mockall::automock` but mockall does not support the
// lifetime bounds on `tokens`: https://github.com/asomers/mockall/issues/134 . As a workaround
// we create a similar trait with simpler lifetimes on which mockall works.
#[cfg(test)]
mod mock {
    use super::*;
    #[mockall::automock]
    pub trait PriceSource_ {
        fn get_prices<'a>(
            &'a self,
            tokens: &[TokenId],
        ) -> BoxFuture<'a, Result<HashMap<TokenId, u128>>>;
    }

    impl PriceSource for MockPriceSource_ {
        fn get_prices(&self, tokens: &[TokenId]) -> BoxFuture<Result<HashMap<TokenId, u128>>> {
            PriceSource_::get_prices(self, tokens)
        }
    }
}
#[cfg(test)]
pub use mock::MockPriceSource_ as MockPriceSource;
