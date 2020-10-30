use super::price_source::PriceSource;
use crate::models::{BatchId, TokenId};
use crate::orderbook::StableXOrderBookReading;
use anyhow::Result;
use pricegraph::Pricegraph;
use std::collections::HashMap;
use std::num::NonZeroU128;
use std::sync::Arc;
use std::time::SystemTime;

pub struct PricegraphEstimator {
    orderbook_reader: Arc<dyn StableXOrderBookReading>,
}

impl PricegraphEstimator {
    pub fn new(orderbook_reader: Arc<dyn StableXOrderBookReading>) -> Self {
        Self { orderbook_reader }
    }
}

#[async_trait::async_trait]
impl PriceSource for PricegraphEstimator {
    async fn get_prices(&self, tokens: &[TokenId]) -> Result<HashMap<TokenId, NonZeroU128>> {
        let batch = BatchId::currently_being_solved(SystemTime::now())?;
        let (account_state, orders) = self
            .orderbook_reader
            .get_auction_data_for_batch(batch.into())
            .await?;
        let pricegraph = Pricegraph::new(orders.iter().map(|order| {
            order.to_element(account_state.read_balance(order.sell_token, order.account_id))
        }));
        pricegraph.get_prices(tokens).await
    }
}

use inner::TokenPriceEstimating;
#[async_trait::async_trait]
impl<T: TokenPriceEstimating> PriceSource for T {
    async fn get_prices(&self, tokens: &[TokenId]) -> Result<HashMap<TokenId, NonZeroU128>> {
        let result = tokens
            .iter()
            .flat_map(|token| {
                let price_in_reference = self.estimate_token_price(*token, None)?;
                Some((*token, NonZeroU128::new(price_in_reference as _)?))
            })
            .collect();
        Ok(result)
    }
}

/// Trait to facilitate testing this module. This is in a private inner module because the trait
/// itself has to be public (`rustc --explain E0445`) but there is no reason for anyone else to
/// implement it.
mod inner {
    use super::{Pricegraph, TokenId};

    #[cfg_attr(test, mockall::automock)]
    pub trait TokenPriceEstimating: Send + Sync {
        fn estimate_token_price(&self, token: TokenId, hops: Option<usize>) -> Option<f64>;
    }

    impl TokenPriceEstimating for Pricegraph {
        fn estimate_token_price(&self, token: TokenId, hops: Option<usize>) -> Option<f64> {
            let estimate = self.estimate_token_price(token.0, hops)?;
            Some(estimate)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::inner::MockTokenPriceEstimating;
    use super::*;
    use futures::FutureExt as _;
    use mockall::predicate::eq;

    const ONE_OWL: f64 = 1_000_000_000_000_000_000.0;

    #[test]
    fn returns_buy_amounts_of_selling_one_owl() {
        let mut pricegraph = MockTokenPriceEstimating::new();
        pricegraph
            .expect_estimate_token_price()
            .with(eq(TokenId(1)), eq(None))
            .return_const(0.5 * ONE_OWL);
        pricegraph
            .expect_estimate_token_price()
            .with(eq(TokenId(2)), eq(None))
            .return_const(2.0 * ONE_OWL);

        let result = pricegraph
            .get_prices(&[1.into(), 2.into()])
            .now_or_never()
            .unwrap()
            .unwrap();
        let expected = hash_map! {
            TokenId(1) => nonzero!(500_000_000_000_000_000),
            TokenId(2) => nonzero!(2_000_000_000_000_000_000),
        };
        assert_eq!(result, expected);
    }

    #[test]
    fn omits_tokens_for_which_estimate_fails() {
        let mut pricegraph = MockTokenPriceEstimating::new();
        pricegraph
            .expect_estimate_token_price()
            .with(eq(TokenId(1)), eq(None))
            .return_const(0.5 * ONE_OWL);
        pricegraph
            .expect_estimate_token_price()
            .with(eq(TokenId(2)), eq(None))
            .return_const(None);

        let result = pricegraph
            .get_prices(&[1.into(), 2.into()])
            .now_or_never()
            .unwrap()
            .unwrap();
        let expected = hash_map! { TokenId(1) => nonzero!(500_000_000_000_000_000) };
        assert_eq!(result, expected);
    }
}
