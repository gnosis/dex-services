use crate::orderbook::Orderbook;
use serde::Serialize;
use serde_with::rust::display_fromstr;
use std::num::ParseIntError;
use std::sync::Arc;
use warp::{Filter, Rejection};

/// Validate a request of the form
/// `/markets/<baseTokenId>-<quoteTokenId>/estimated-buy-amount/<sellAmountInQuoteToken>`
/// and answer it.
pub fn estimated_buy_amount<T: Send + Sync + 'static>(
    orderbook: Arc<Orderbook<T>>,
    price_rounding_buffer: f64,
) -> impl Filter<Extract = (String,), Error = Rejection> + Clone {
    estimated_buy_amount_filter().and_then({
        move |token_pair, sell_amount_in_quote| {
            let orderbook = orderbook.clone();
            async move {
                let orderbook = orderbook.get_reduced_orderbook().await;
                let result = estimate_buy_amount(
                    token_pair,
                    sell_amount_in_quote,
                    price_rounding_buffer,
                    orderbook,
                );
                // The compiler cannot infer the error type because we never return an error.
                Result::<_, Rejection>::Ok(result)
            }
        }
    })
}

fn estimated_buy_amount_filter(
) -> impl Filter<Extract = (TokenPair, u128), Error = Rejection> + Copy {
    warp::path!("markets" / TokenPair / "estimated-buy-amount" / u128)
}

fn estimate_buy_amount(
    token_pair: TokenPair,
    sell_amount_in_quote: u128,
    price_rounding_buffer: f64,
    orderbook: pricegraph::Orderbook,
) -> String {
    let buy_amount_in_base = crate::estimate_buy_amount::estimate_buy_amount(
        token_pair,
        sell_amount_in_quote as f64,
        price_rounding_buffer,
        orderbook,
    )
    .unwrap_or(0.0) as u128;
    serde_json::to_string(&JsonResult {
        base_token_id: token_pair.buy_token_id,
        quote_token_id: token_pair.sell_token_id,
        sell_amount_in_quote,
        buy_amount_in_base,
    })
    .expect("serialization failed")
}

#[derive(Debug, Copy, Clone)]
pub struct TokenPair {
    pub buy_token_id: u16,
    pub sell_token_id: u16,
}

impl std::convert::Into<pricegraph::TokenPair> for TokenPair {
    fn into(self) -> pricegraph::TokenPair {
        pricegraph::TokenPair {
            buy: self.buy_token_id,
            sell: self.sell_token_id,
        }
    }
}

#[derive(thiserror::Error, Debug)]
pub enum ParseTokenPairError {
    #[error("wrong number of tokens")]
    WrongNumberOfTokens,
    #[error("parse int error")]
    ParseIntError(#[from] ParseIntError),
}

impl std::str::FromStr for TokenPair {
    type Err = ParseTokenPairError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let mut split = s.split('-');
        let mut next_token_id = || -> Result<u16, ParseTokenPairError> {
            let token_string = split
                .next()
                .ok_or(ParseTokenPairError::WrongNumberOfTokens)?;
            token_string.parse().map_err(From::from)
        };
        let buy_token_id = next_token_id()?;
        let sell_token_id = next_token_id()?;
        if split.next().is_some() {
            return Err(ParseTokenPairError::WrongNumberOfTokens);
        }
        Ok(Self {
            buy_token_id,
            sell_token_id,
        })
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct JsonResult {
    #[serde(with = "display_fromstr")]
    base_token_id: u16,
    #[serde(with = "display_fromstr")]
    quote_token_id: u16,
    #[serde(with = "display_fromstr")]
    buy_amount_in_base: u128,
    #[serde(with = "display_fromstr")]
    sell_amount_in_quote: u128,
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::future::FutureExt as _;
    use serde_json::Value;

    #[test]
    fn estimated_buy_amount_ok() {
        let (token_pair, volume) = warp::test::request()
            .path("/markets/0-65535/estimated-buy-amount/1")
            .filter(&estimated_buy_amount_filter())
            .now_or_never()
            .unwrap()
            .unwrap();
        assert_eq!(token_pair.buy_token_id, 0);
        assert_eq!(token_pair.sell_token_id, 65535);
        assert_eq!(volume, 1);
    }

    #[test]
    fn estimated_buy_amount_too_few_tokens() {
        for path in &[
            "/markets//estimated-buy-amount/1",
            "/markets/0/estimated-buy-amount/1",
        ] {
            assert!(warp::test::request()
                .path(path)
                .filter(&estimated_buy_amount_filter())
                .now_or_never()
                .unwrap()
                .is_err());
        }
    }

    #[test]
    fn estimated_buy_amount_too_many_tokens() {
        for path in &[
            "/markets/0-1-2/estimated-buy-amount/1",
            "/markets/0-1-asdf/estimated-buy-amount/1",
            "/markets/0-1-2-3/estimated-buy-amount/1",
            "/markets/0-1-/estimated-buy-amount/1",
        ] {
            assert!(warp::test::request()
                .path(path)
                .filter(&estimated_buy_amount_filter())
                .now_or_never()
                .unwrap()
                .is_err());
        }
    }

    #[test]
    fn estimated_buy_amount_no_sell_amount() {
        for path in &[
            "/markets/0-1/estimated-buy-amount/",
            "/markets/0-1/estimated-buy-amount/asdf",
        ] {
            assert!(warp::test::request()
                .path(path)
                .filter(&estimated_buy_amount_filter())
                .now_or_never()
                .unwrap()
                .is_err());
        }
    }

    #[test]
    fn estimated_buy_amount_no_float_volume() {
        for path in &[
            "/markets/0-1/estimated-buy-amount/0.0",
            "/markets/0-1/estimated-buy-amount/1.0",
            "/markets/0-1/estimated-buy-amount/0.5",
        ] {
            assert!(warp::test::request()
                .path(path)
                .filter(&estimated_buy_amount_filter())
                .now_or_never()
                .unwrap()
                .is_err());
        }
    }

    #[test]
    fn serialization() {
        let original = JsonResult {
            base_token_id: 1,
            quote_token_id: 2,
            buy_amount_in_base: 3,
            sell_amount_in_quote: 4,
        };
        let serialized = serde_json::to_string(&original).unwrap();
        let json: Value = serde_json::from_str(&serialized).unwrap();
        let expected = serde_json::json!({
            "baseTokenId": "1",
            "quoteTokenId": "2",
            "buyAmountInBase": "3",
            "sellAmountInQuote": "4",
        });
        assert_eq!(json, expected);
    }
}
