use super::price_source::PriceSource;
use crate::models::{BatchId, TokenId};
use crate::orderbook::StableXOrderBookReading;
use anyhow::Result;
use futures::future::{BoxFuture, FutureExt as _};
use pricegraph::{Pricegraph, TokenPair};
use std::collections::HashMap;
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

const ONE_OWL: f64 = 1_000_000_000_000_000_000.0;

impl PriceSource for PricegraphEstimator {
    fn get_prices<'a>(
        &'a self,
        tokens: &'a [TokenId],
    ) -> BoxFuture<'a, Result<HashMap<TokenId, u128>>> {
        async move {
            let batch = BatchId::currently_being_solved(SystemTime::now())?;
            let (account_state, orders) =
                self.orderbook_reader.get_auction_data(batch.into()).await?;
            let pricegraph = Pricegraph::new(orders.iter().map(|order| {
                order.to_element(account_state.read_balance(order.sell_token, order.account_id))
            }));
            pricegraph.get_prices(tokens).await
        }
        .boxed()
    }
}

use inner::LimitPriceEstimating;
impl<T: LimitPriceEstimating> PriceSource for T {
    fn get_prices<'a>(
        &'a self,
        tokens: &'a [TokenId],
    ) -> BoxFuture<'a, Result<HashMap<TokenId, u128>>> {
        let result = tokens
            .iter()
            .flat_map(|token| {
                let price_in_token = if token == &TokenId::reference() {
                    1.0
                } else {
                    // Estimate price by selling 1 unit of the reference token for each token.
                    // We sell rather than buy the reference token because volume is denominated
                    // in the sell token, for which we know the number of decimals.
                    let pair = TokenPair {
                        buy: token.0,
                        sell: TokenId::reference().0,
                    };
                    self.estimate_limit_price(pair, ONE_OWL)?
                };
                let price_in_reference = 1.0 / price_in_token;
                Some((*token, (ONE_OWL * price_in_reference) as u128))
            })
            .collect();
        immediate!(Ok(result))
    }
}

/// Trait to facilitate testing this module. This is in a private inner module because the trait
/// itself has to be public (`rustc --explain E0445`) but there is no reason for anyone else to
/// implement it.
mod inner {
    use pricegraph::{Pricegraph, TokenPair};

    #[cfg_attr(test, mockall::automock)]
    pub trait LimitPriceEstimating {
        fn estimate_limit_price(&self, pair: TokenPair, sell_amount: f64) -> Option<f64>;
    }

    impl LimitPriceEstimating for Pricegraph {
        fn estimate_limit_price(&self, pair: TokenPair, sell_amount: f64) -> Option<f64> {
            self.estimate_limit_price(pair, sell_amount)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use inner::MockLimitPriceEstimating;
    use mockall::predicate::eq;

    #[test]
    fn returns_buy_amounts_of_selling_one_owl() {
        let mut pricegraph = MockLimitPriceEstimating::new();
        pricegraph
            .expect_estimate_limit_price()
            .with(eq(TokenPair { buy: 1, sell: 0 }), eq(ONE_OWL))
            .return_const(2.0);
        pricegraph
            .expect_estimate_limit_price()
            .with(eq(TokenPair { buy: 2, sell: 0 }), eq(ONE_OWL))
            .return_const(0.5);

        let result = pricegraph
            .get_prices(&[1.into(), 2.into()])
            .now_or_never()
            .unwrap()
            .unwrap();
        let expected = hash_map! {
            TokenId(1) => 500_000_000_000_000_000,
            TokenId(2) => 2_000_000_000_000_000_000,
        };
        assert_eq!(result, expected);
    }

    #[test]
    fn omits_tokens_for_which_estimate_fails() {
        let mut pricegraph = MockLimitPriceEstimating::new();
        pricegraph
            .expect_estimate_limit_price()
            .with(eq(TokenPair { buy: 1, sell: 0 }), eq(ONE_OWL))
            .return_const(0.5);
        pricegraph
            .expect_estimate_limit_price()
            .with(eq(TokenPair { buy: 2, sell: 0 }), eq(ONE_OWL))
            .return_const(None);

        let result = pricegraph
            .get_prices(&[1.into(), 2.into()])
            .now_or_never()
            .unwrap()
            .unwrap();
        let expected = hash_map! { TokenId(1) => 2_000_000_000_000_000_000 };
        assert_eq!(result, expected);
    }

    #[test]
    fn returns_one_owl_for_estimating_owl() {
        let pricegraph = MockLimitPriceEstimating::new();

        let result = pricegraph
            .get_prices(&[0.into()])
            .now_or_never()
            .unwrap()
            .unwrap();
        let expected = hash_map! { TokenId(0) => 1_000_000_000_000_000_000 };
        assert_eq!(result, expected);
    }
}
