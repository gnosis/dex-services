use super::PriceSource;
use crate::models::TokenId;
use anyhow::Result;
use futures::future::{BoxFuture, FutureExt as _};
use std::collections::hash_map::RandomState;
use std::collections::{HashMap, HashSet};
use std::iter::FromIterator;

/**
 * A price source that sequentially queries its inner sources in order and returns the
 * first price found.
 */
pub struct PriorityPriceSource {
    sources: Vec<Box<dyn PriceSource + Send + Sync>>,
}

impl PriorityPriceSource {
    #[allow(dead_code)]
    pub fn new(sources: Vec<Box<dyn PriceSource + Send + Sync>>) -> Self {
        Self { sources }
    }
}

impl PriceSource for PriorityPriceSource {
    fn get_prices<'a>(
        &'a self,
        tokens: &'a [TokenId],
    ) -> BoxFuture<'a, Result<HashMap<TokenId, u128>>> {
        async move {
            let mut remaining_tokens: HashSet<TokenId, RandomState> =
                HashSet::from_iter(tokens.iter().cloned());
            let mut result = HashMap::new();
            for source in &self.sources {
                let remaining_token_vec = Vec::from_iter(remaining_tokens.iter().cloned());
                match source.get_prices(&remaining_token_vec).await {
                    Ok(partial_result) => {
                        remaining_tokens = remaining_tokens
                            .into_iter()
                            .filter(|token| !partial_result.contains_key(token))
                            .collect();
                        result.extend(partial_result);
                    }
                    Err(err) => log::warn!("Price Source failed: {:?}", err),
                };
                if remaining_tokens.is_empty() {
                    break;
                }
            }
            Ok(result)
        }
        .boxed()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::price_estimation::price_source::MockPriceSource;
    use anyhow::anyhow;

    #[test]
    fn returns_price_from_first_available_source() {
        let mut first_source = MockPriceSource::new();
        let mut second_source = MockPriceSource::new();

        first_source
            .expect_get_prices()
            .times(1)
            .withf(|token| token == &[TokenId::from(1), TokenId::from(2)][..])
            .returning(|_| {
                immediate!(Ok(hash_map! {
                        TokenId::from(1) => 100
                }))
            });
        // Expect second source to be called with missing tokens
        second_source
            .expect_get_prices()
            .times(1)
            .withf(|token| token == &[TokenId::from(2)][..])
            .returning(|_| {
                immediate!(Ok(hash_map! {
                    TokenId::from(2) => 50
                }))
            });

        let priority_source =
            PriorityPriceSource::new(vec![Box::new(first_source), Box::new(second_source)]);
        priority_source
            .get_prices(&[1.into(), 2.into()])
            .now_or_never();
    }

    #[test]
    fn skips_failing_sources() {
        let mut first_source = MockPriceSource::new();
        let mut second_source = MockPriceSource::new();

        first_source
            .expect_get_prices()
            .returning(|_| immediate!(Err(anyhow!("Error"))));
        second_source.expect_get_prices().returning(|_| {
            immediate!(Ok(hash_map! {
                TokenId::from(1) => 50
            }))
        });

        let priority_source =
            PriorityPriceSource::new(vec![Box::new(first_source), Box::new(second_source)]);
        assert_eq!(
            priority_source
                .get_prices(&[1.into()])
                .now_or_never()
                .unwrap()
                .unwrap(),
            hash_map! {
                TokenId::from(1) => 50
            }
        );
    }

    #[test]
    fn omits_tokens_for_which_no_prices_exist() {
        let mut source = MockPriceSource::new();

        source.expect_get_prices().returning(|_| {
            immediate!(Ok(hash_map! {
                TokenId::from(2) => 50
            }))
        });

        let priority_source = PriorityPriceSource::new(vec![Box::new(source)]);
        assert_eq!(
            priority_source
                .get_prices(&[1.into(), 2.into()])
                .now_or_never()
                .unwrap()
                .unwrap(),
            hash_map! {
                TokenId::from(2) => 50
            }
        );
    }
}
