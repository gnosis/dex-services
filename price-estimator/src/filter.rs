use crate::models::*;
use crate::orderbook::Orderbook;
use std::convert::Infallible;
use std::sync::Arc;
use warp::{Filter, Rejection};

/// Handles all supported requests.
pub fn all<T: Send + Sync + 'static>(
    orderbook: Arc<Orderbook<T>>,
    price_rounding_buffer: f64,
) -> impl Filter<Extract = impl warp::Reply, Error = Rejection> + Clone {
    markets(orderbook.clone()).or(estimated_buy_amount(orderbook, price_rounding_buffer))
}

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
        .with(warp::log("price_estimator::api::estimate_buy_amount"))
}

/// Validate a request of the form
/// `/markets/<baseTokenId>-<quoteTokenId>`
/// and answer it.
pub fn markets<T: Send + Sync + 'static>(
    orderbook: Arc<Orderbook<T>>,
) -> impl Filter<Extract = impl warp::Reply, Error = Rejection> + Clone {
    markets_filter()
        .and(warp::any().map(move || orderbook.clone()))
        .and_then(get_markets)
        .with(warp::log("price_estimator::api::markets"))
}

fn estimated_buy_amount_filter(
) -> impl Filter<Extract = (TokenPair, u128, QueryParameters), Error = Rejection> + Copy {
    warp::path!("markets" / TokenPair / "estimated-buy-amount" / u128)
        .and(warp::get())
        .and(warp::query::<QueryParameters>())
}

fn markets_filter() -> impl Filter<Extract = (TokenPair, QueryParameters), Error = Rejection> + Copy
{
    warp::path!("markets" / TokenPair)
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
    let transitive_order = orderbook
        .get_pricegraph()
        .await
        .order_for_sell_amount(token_pair.into(), sell_amount_in_quote as f64);
    let buy_amount_in_base = if let Some(order) = transitive_order {
        (1.0 - price_rounding_buffer) * order.buy
    } else {
        0.0
    } as u128;
    let result = EstimatedBuyAmountResult {
        base_token_id: token_pair.buy_token_id,
        quote_token_id: token_pair.sell_token_id,
        sell_amount_in_quote,
        buy_amount_in_base,
    };
    Ok(warp::reply::json(&result))
}

async fn get_markets<T>(
    token_pair: TokenPair,
    _query: QueryParameters,
    orderbook: Arc<Orderbook<T>>,
) -> Result<impl warp::Reply, Infallible> {
    let transitive_orderbook = orderbook.get_pricegraph().await.transitive_orderbook(
        token_pair.buy_token_id,
        token_pair.sell_token_id,
        None,
    );
    let result = MarketsResult::from(&transitive_orderbook);
    Ok(warp::reply::json(&result))
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::future::FutureExt as _;

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
    fn markets_ok() {
        let (token_pair, query) = warp::test::request()
            .path("/markets/1-2?atoms=true&hops=3")
            .filter(&markets_filter())
            .now_or_never()
            .unwrap()
            .unwrap();
        assert_eq!(token_pair.buy_token_id, 1);
        assert_eq!(token_pair.sell_token_id, 2);
        assert_eq!(query.atoms, true);
        assert_eq!(query.hops, Some(3));
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
}
