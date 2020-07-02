use super::PriceSource;
use crate::models::TokenId;
use anyhow::Result;
use futures::future::{BoxFuture, FutureExt as _};
use std::collections::HashMap;

/**
 * A price source that sequentially queries its inner sources in order and returns the
 * first price found.
 */
pub struct PriorityPriceSource {
    sources: Vec<Box<dyn PriceSource + Send + Sync>>,
}

impl PriorityPriceSource {
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
            let mut remaining_tokens = tokens.to_vec();
            let mut result = HashMap::new();
            for source in &self.sources {
                match source.get_prices(&remaining_tokens).await {
                    Ok(partial_result) => {
                        remaining_tokens.retain(|token| !partial_result.contains_key(token));
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
