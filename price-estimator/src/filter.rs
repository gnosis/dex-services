use crate::orderbook::Orderbook;
use serde::{Deserialize, Serialize};
use serde_with::rust::display_fromstr;
use std::convert::Infallible;
use std::num::ParseIntError;
use std::sync::Arc;
use warp::{Filter, Rejection};

/// Validate a request of the form
/// `/markets/<baseTokenId>-<quoteTokenId>/estimated-buy-amount/<sellAmountInQuoteToken>`
/// and answer it.
pub fn estimated_buy_amount<T: Send + Sync + 'static>(
    orderbook: Arc<Orderbook<T>>,
    price_rounding_buffer: f64,
) -> impl Filter<Extract = impl warp::Reply, Error = Rejection> + Clone {
    estimated_buy_amount_filter()
        .and(warp::any().map(move || price_rounding_buffer))
        .and(warp::any().map(move || orderbook.clone()))
        .and_then(estimate_buy_amount)
}

fn estimated_buy_amount_filter(
) -> impl Filter<Extract = (TokenPair, u128, QueryParameters), Error = Rejection> + Copy {
    warp::path!("markets" / TokenPair / "estimated-buy-amount" / u128)
        .and(warp::get())
        .and(warp::query::<QueryParameters>())
}

async fn estimate_buy_amount<T>(
    token_pair: TokenPair,
    sell_amount_in_quote: u128,
    _query: QueryParameters,
    price_rounding_buffer: f64,
    orderbook: Arc<Orderbook<T>>,
) -> Result<impl warp::Reply, Infallible> {
    let buy_amount_in_base = crate::estimate_buy_amount::estimate_buy_amount(
        token_pair,
        sell_amount_in_quote as f64,
        price_rounding_buffer,
        orderbook.get_reduced_orderbook().await,
    )
    .unwrap_or(0.0) as u128;
    let result = JsonResult {
        base_token_id: token_pair.buy_token_id,
        quote_token_id: token_pair.sell_token_id,
        sell_amount_in_quote,
        buy_amount_in_base,
    };
    Ok(warp::reply::json(&result))
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

#[derive(Debug, Deserialize)]
struct QueryParameters {
    atoms: bool,
    hops: Option<u16>,
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
        let (token_pair, volume, query) = warp::test::request()
            .path("/markets/0-65535/estimated-buy-amount/1?atoms=true&hops=2")
            .filter(&estimated_buy_amount_filter())
            .now_or_never()
            .unwrap()
            .unwrap();
        assert_eq!(token_pair.buy_token_id, 0);
        assert_eq!(token_pair.sell_token_id, 65535);
        assert_eq!(volume, 1);
        assert_eq!(query.atoms, true);
        assert_eq!(query.hops, Some(2));
    }

    #[test]
    fn missing_hops_ok() {
        let (_, _, query) = warp::test::request()
            .path("/markets/0-65535/estimated-buy-amount/1?atoms=true")
            .filter(&estimated_buy_amount_filter())
            .now_or_never()
            .unwrap()
            .unwrap();
        assert_eq!(query.hops, None);
    }

    #[test]
    fn missing_query() {
        assert!(warp::test::request()
            .path("/markets/0-0/estimated-buy-amount/0")
            .filter(&estimated_buy_amount_filter())
            .now_or_never()
            .unwrap()
            .is_err());
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
