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
            Ok(self.estimate_prices(tokens, pricegraph))
        }
        .boxed()
    }
}

/// Private trait to facilitate testing this module
#[cfg_attr(test, mockall::automock)]
trait PriceEstimating {
    fn estimate_price(&self, pair: TokenPair, volume: f64) -> Option<f64>;
}

impl PriceEstimating for Pricegraph {
    fn estimate_price(&self, pair: TokenPair, volume: f64) -> Option<f64> {
        self.estimate_limit_price(pair, volume)
    }
}

impl PricegraphEstimator {
    pub fn new(orderbook_reader: Arc<dyn StableXOrderBookReading>) -> Self {
        Self { orderbook_reader }
    }

    fn estimate_prices(
        &self,
        tokens: &[TokenId],
        pricegraph: impl PriceEstimating,
    ) -> HashMap<TokenId, u128> {
        tokens
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
                    pricegraph.estimate_price(pair, ONE_OWL)?
                };
                let price_in_reference = 1.0 / price_in_token;
                Some((*token, (ONE_OWL * price_in_reference) as u128))
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::orderbook::MockStableXOrderBookReading;
    use mockall::predicate::eq;

    #[test]
    fn returns_buy_amounts_of_selling_one_owl() {
        let mut pricegraph = MockPriceEstimating::new();
        pricegraph
            .expect_estimate_price()
            .with(eq(TokenPair { buy: 1, sell: 0 }), eq(ONE_OWL))
            .return_const(2.0);
        pricegraph
            .expect_estimate_price()
            .with(eq(TokenPair { buy: 2, sell: 0 }), eq(ONE_OWL))
            .return_const(0.5);

        let reader = Arc::new(MockStableXOrderBookReading::new());
        let estimator = PricegraphEstimator::new(reader);
        assert_eq!(
            estimator.estimate_prices(&[1.into(), 2.into()], pricegraph),
            hash_map! {
                TokenId(1) => 500_000_000_000_000_000,
                TokenId(2) => 2_000_000_000_000_000_000,
            }
        );
    }

    #[test]
    fn omits_tokens_for_which_estimate_fails() {
        let mut pricegraph = MockPriceEstimating::new();
        pricegraph
            .expect_estimate_price()
            .with(eq(TokenPair { buy: 1, sell: 0 }), eq(ONE_OWL))
            .return_const(0.5);
        pricegraph
            .expect_estimate_price()
            .with(eq(TokenPair { buy: 2, sell: 0 }), eq(ONE_OWL))
            .return_const(None);

        let reader = Arc::new(MockStableXOrderBookReading::new());
        let estimator = PricegraphEstimator::new(reader);
        assert_eq!(
            estimator.estimate_prices(&[1.into(), 2.into()], pricegraph),
            hash_map! {
                TokenId(1) => 2_000_000_000_000_000_000,
            }
        );
    }

    #[test]
    fn returns_one_owl_for_estimating_owl() {
        let pricegraph = MockPriceEstimating::new();
        let reader = Arc::new(MockStableXOrderBookReading::new());
        let estimator = PricegraphEstimator::new(reader);
        assert_eq!(
            estimator.estimate_prices(&[0.into()], pricegraph),
            hash_map! {
                TokenId(0) => 1_000_000_000_000_000_000,
            }
        );
    }
}
