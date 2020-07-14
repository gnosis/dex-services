use crate::models::*;
use crate::orderbook::Orderbook;
use core::{
    models::TokenId,
    token_info::{TokenBaseInfo, TokenInfoFetching},
};
use pricegraph::TokenPair;
use std::convert::Infallible;
use std::sync::Arc;
use warp::{
    http::StatusCode,
    reject::{self, Reject},
    Filter, Rejection, Reply,
};

/// Handles all supported requests under a `/api/v1` root path.
pub fn all<T: Send + Sync + 'static>(
    orderbook: Arc<Orderbook<T>>,
    token_info: Arc<dyn TokenInfoFetching>,
    price_rounding_buffer: f64,
) -> impl Filter<Extract = impl Reply, Error = Infallible> + Clone {
    warp::path!("api" / "v1" / ..)
        .and(
            markets(orderbook.clone())
                .or(estimated_buy_amount(
                    orderbook.clone(),
                    token_info.clone(),
                    price_rounding_buffer,
                ))
                .or(estimated_amounts_at_price(
                    orderbook.clone(),
                    token_info,
                    price_rounding_buffer,
                ))
                .or(estimated_best_ask_price(orderbook, price_rounding_buffer)),
        )
        .recover(handle_rejection)
}

#[derive(Debug)]
struct NoTokenInfo;
impl Reject for NoTokenInfo {}

async fn handle_rejection(err: Rejection) -> Result<impl Reply, Infallible> {
    let code;
    let message;

    if err.is_not_found() {
        code = StatusCode::NOT_FOUND;
        message = "invalid url path";
    } else if let Some(NoTokenInfo) = err.find() {
        code = StatusCode::BAD_REQUEST;
        message = "request with atoms=true for token we don't have erc20 info for";
    } else if let Some(warp::reject::InvalidQuery { .. }) = err.find() {
        code = StatusCode::BAD_REQUEST;
        message = "invalid url query";
    } else {
        log::warn!("unhandled rejection: {:?}", err);
        code = StatusCode::INTERNAL_SERVER_ERROR;
        message = "unexpected internal error";
    }

    let json = warp::reply::json(&ErrorResult { message });
    Ok(warp::reply::with_status(json, code))
}

/// Validate a request of the form
/// `/markets/<baseTokenId>-<quoteTokenId>`
/// and answer it.
fn markets<T: Send + Sync + 'static>(
    orderbook: Arc<Orderbook<T>>,
) -> impl Filter<Extract = impl Reply, Error = Rejection> + Clone {
    markets_filter()
        .and(warp::any().map(move || orderbook.clone()))
        .and_then(get_markets)
        .with(warp::log("price_estimator::api::markets"))
}

/// Validate a request of the form
/// `/markets/<baseTokenId>-<quoteTokenId>/estimated-buy-amount/<sellAmountInQuoteToken>`
/// and answer it.
fn estimated_buy_amount<T: Send + Sync + 'static>(
    orderbook: Arc<Orderbook<T>>,
    token_info: Arc<dyn TokenInfoFetching>,
    price_rounding_buffer: f64,
) -> impl Filter<Extract = impl Reply, Error = Rejection> + Clone {
    estimated_buy_amount_filter()
        .and(warp::any().map(move || price_rounding_buffer))
        .and(warp::any().map(move || orderbook.clone()))
        .and(warp::any().map(move || token_info.clone()))
        .and_then(estimate_buy_amount)
        .with(warp::log("price_estimator::api::estimate_buy_amount"))
}

/// Validate a request of the form:
/// `/markets/<baseTokenId>-<quoteTokenId>/estimated-amounts-at-price/<exchangeRate>`
/// and answer it.
fn estimated_amounts_at_price<T: Send + Sync + 'static>(
    orderbook: Arc<Orderbook<T>>,
    token_info: Arc<dyn TokenInfoFetching>,
    price_rounding_buffer: f64,
) -> impl Filter<Extract = impl Reply, Error = Rejection> + Clone {
    estimated_amounts_at_price_filter()
        .and(warp::any().map(move || price_rounding_buffer))
        .and(warp::any().map(move || orderbook.clone()))
        .and(warp::any().map(move || token_info.clone()))
        .and_then(estimate_amounts_at_price)
        .with(warp::log("price_estimator::api::estimate_amounts_at_price"))
}

/// Validate a request of the form:
/// `/markets/<baseTokenId>-<quoteTokenId>/estimated-amounts-at-price/<exchangeRate>`
/// and answer it.
fn estimated_best_ask_price<T: Send + Sync + 'static>(
    orderbook: Arc<Orderbook<T>>,
    price_rounding_buffer: f64,
) -> impl Filter<Extract = impl Reply, Error = Rejection> + Clone {
    estimated_best_ask_price_filter()
        .and(warp::any().map(move || price_rounding_buffer))
        .and(warp::any().map(move || orderbook.clone()))
        .and_then(estimate_best_ask_price)
        .with(warp::log("price_estimator::api::estimate_best_ask_price"))
}

fn markets_prefix() -> impl Filter<Extract = (Market,), Error = Rejection> + Copy {
    warp::path!("markets" / Market / ..)
}

fn markets_bid_prefix() -> impl Filter<Extract = (TokenPair,), Error = Rejection> + Copy {
    markets_prefix().map(|market: Market| market.bid_pair())
}

fn markets_filter() -> impl Filter<Extract = (Market, QueryParameters), Error = Rejection> + Copy {
    markets_prefix()
        .and(warp::path::end())
        .and(warp::get())
        .and(warp::query::<QueryParameters>())
}

fn estimated_buy_amount_filter(
) -> impl Filter<Extract = (TokenPair, f64, QueryParameters), Error = Rejection> + Copy {
    markets_bid_prefix()
        .and(warp::path!("estimated-buy-amount" / f64))
        .and(warp::get())
        .and(warp::query::<QueryParameters>())
}

fn estimated_amounts_at_price_filter(
) -> impl Filter<Extract = (TokenPair, f64, QueryParameters), Error = Rejection> + Copy {
    markets_bid_prefix()
        .and(warp::path!("estimated-amounts-at-price" / f64))
        .and(warp::get())
        .and(warp::query::<QueryParameters>())
}

fn estimated_best_ask_price_filter(
) -> impl Filter<Extract = (TokenPair, QueryParameters), Error = Rejection> + Copy {
    markets_bid_prefix()
        .and(warp::path!("estimated-best-ask-price"))
        .and(warp::get())
        .and(warp::query::<QueryParameters>())
}

async fn get_markets<T>(
    market: Market,
    _query: QueryParameters,
    orderbook: Arc<Orderbook<T>>,
) -> Result<impl Reply, Infallible> {
    let transitive_orderbook = orderbook
        .get_pricegraph()
        .await
        .transitive_orderbook(*market, None);
    let result = MarketsResult::from(&transitive_orderbook);
    Ok(warp::reply::json(&result))
}

async fn get_token_info(
    token_id: u16,
    token_info_fetching: &dyn TokenInfoFetching,
) -> Result<TokenBaseInfo, Rejection> {
    token_info_fetching
        .get_token_info(TokenId(token_id))
        .await
        .map_err(|_| reject::custom(NoTokenInfo))
}

async fn estimate_buy_amount<T>(
    token_pair: TokenPair,
    sell_amount_in_quote: f64,
    query: QueryParameters,
    price_rounding_buffer: f64,
    orderbook: Arc<Orderbook<T>>,
    token_infos: Arc<dyn TokenInfoFetching>,
) -> Result<impl Reply, Rejection> {
    let sell_amount_in_quote_atoms = if query.atoms {
        sell_amount_in_quote
    } else {
        let token_info = get_token_info(token_pair.sell, token_infos.as_ref()).await?;
        sell_amount_in_quote * token_info.base_unit_in_atoms()
    };
    let transitive_order = orderbook
        .get_pricegraph()
        .await
        .order_for_sell_amount(token_pair, sell_amount_in_quote_atoms);
    let buy_amount_in_base_atoms = transitive_order
        .map(|order| apply_rounding_buffer(order.buy, price_rounding_buffer))
        .unwrap_or_default();
    let buy_amount_in_base = if query.atoms {
        buy_amount_in_base_atoms
    } else {
        let token_info = get_token_info(token_pair.buy, token_infos.as_ref()).await?;
        buy_amount_in_base_atoms / token_info.base_unit_in_atoms()
    };
    let result = EstimatedOrderResult {
        base_token_id: token_pair.buy,
        quote_token_id: token_pair.sell,
        sell_amount_in_quote,
        buy_amount_in_base,
    };
    Ok(warp::reply::json(&result))
}

async fn estimate_amounts_at_price<T>(
    token_pair: TokenPair,
    price_in_quote: f64,
    query: QueryParameters,
    price_rounding_buffer: f64,
    orderbook: Arc<Orderbook<T>>,
    token_infos: Arc<dyn TokenInfoFetching>,
) -> Result<impl Reply, Rejection> {
    let result = if query.atoms {
        estimate_amounts_at_price_atoms(
            token_pair,
            price_in_quote,
            price_rounding_buffer,
            orderbook,
        )
        .await
    } else {
        let buy_token_info = get_token_info(token_pair.buy, token_infos.as_ref()).await?;
        let sell_token_info = get_token_info(token_pair.sell, token_infos.as_ref()).await?;
        let price_in_quote_atoms = price_in_quote
            * (sell_token_info.base_unit_in_atoms() / buy_token_info.base_unit_in_atoms());
        let mut result = estimate_amounts_at_price_atoms(
            token_pair,
            price_in_quote_atoms,
            price_rounding_buffer,
            orderbook,
        )
        .await;
        result.buy_amount_in_base /= buy_token_info.base_unit_in_atoms();
        result.sell_amount_in_quote /= sell_token_info.base_unit_in_atoms();
        result
    };
    Ok(warp::reply::json(&result))
}

/// Like `estimate_amounts_at_price` but the price is given and returned in atoms.
async fn estimate_amounts_at_price_atoms<T>(
    token_pair: TokenPair,
    price_in_quote: f64,
    price_rounding_buffer: f64,
    orderbook: Arc<Orderbook<T>>,
) -> EstimatedOrderResult {
    // NOTE: The price in quote is `sell_amount / buy_amount` which is the
    // inverse of an exchange rate. Additionally, we need to apply the price
    // rounding buffer to the price, which will **increase** the exchange rate,
    // making it more restrictive and the estimate more pessimistic.
    let exchange_rate = 1.0 / apply_rounding_buffer(price_in_quote, price_rounding_buffer);
    let transitive_order = orderbook
        .get_pricegraph()
        .await
        .order_at_exchange_rate(token_pair, exchange_rate);
    let (buy_amount_in_base, sell_amount_in_quote) = transitive_order
        .map(|order| {
            (
                apply_rounding_buffer(order.buy, price_rounding_buffer),
                order.sell,
            )
        })
        .unwrap_or_default();
    EstimatedOrderResult {
        base_token_id: token_pair.buy,
        quote_token_id: token_pair.sell,
        sell_amount_in_quote,
        buy_amount_in_base,
    }
}

async fn estimate_best_ask_price<T>(
    token_pair: TokenPair,
    _query: QueryParameters,
    price_rounding_buffer: f64,
    orderbook: Arc<Orderbook<T>>,
) -> Result<impl Reply, Infallible> {
    let price = orderbook
        .get_pricegraph()
        .await
        .estimate_exchange_rate(token_pair, 0.0)
        .map(|xrate| {
            // NOTE: Exchange rate is the inverse of price for an ask order.
            1.0 / apply_rounding_buffer(xrate, price_rounding_buffer)
        });

    let result = PriceEstimateResult(price);
    Ok(warp::reply::json(&result))
}

fn apply_rounding_buffer(amount: f64, price_rounding_buffer: f64) -> f64 {
    ((1.0 - price_rounding_buffer) * amount) as _
}

#[cfg(test)]
mod tests {
    use super::*;
    use anyhow::{anyhow, Result};
    use core::orderbook::StableXOrderBookReading;
    use futures::future::{BoxFuture, FutureExt as _};

    fn empty_orderbook() -> impl StableXOrderBookReading {
        struct OrderbookReader {}
        impl StableXOrderBookReading for OrderbookReader {
            fn get_auction_data<'a>(
                &'a self,
                _: u32,
            ) -> BoxFuture<'a, Result<(core::models::AccountState, Vec<core::models::Order>)>>
            {
                async { Ok(Default::default()) }.boxed()
            }
        }
        OrderbookReader {}
    }

    fn empty_token_info() -> impl TokenInfoFetching {
        struct TokenInfoFetcher {}
        impl TokenInfoFetching for TokenInfoFetcher {
            fn get_token_info<'a>(
                &'a self,
                _: TokenId,
            ) -> BoxFuture<'a, Result<core::token_info::TokenBaseInfo>> {
                async { Err(anyhow!("")) }.boxed()
            }
            fn all_ids<'a>(&'a self) -> BoxFuture<'a, Result<Vec<TokenId>>> {
                async { Ok(Default::default()) }.boxed()
            }
        }
        TokenInfoFetcher {}
    }

    fn all_filter() -> impl Filter<Extract = impl Reply, Error = Infallible> + Clone {
        let orderbook = Arc::new(Orderbook::new(empty_orderbook()));
        let token_info = Arc::new(empty_token_info());
        all(orderbook, token_info, 0.0)
    }

    #[test]
    fn error_unhandled_path() {
        let response = warp::test::request()
            .path("/")
            .reply(&all_filter())
            .now_or_never()
            .unwrap();
        assert_eq!(response.status(), 404);
    }

    #[test]
    fn error_no_token_info() {
        let response = warp::test::request()
            .path("/api/v1/markets/0-1/estimated-buy-amount/2?atoms=false&hops=3")
            .reply(&all_filter())
            .now_or_never()
            .unwrap();
        assert_eq!(response.status(), 400);
    }

    #[test]
    fn all_filter_ok() {
        let response = warp::test::request()
            .path("/api/v1/markets/0-1/estimated-buy-amount/2?atoms=true&hops=3")
            .reply(&all_filter())
            .now_or_never()
            .unwrap();
        assert_eq!(response.status(), 200);
    }

    #[test]
    #[allow(clippy::float_cmp)]
    fn estimated_buy_amount_ok() {
        let (token_pair, volume, query) = warp::test::request()
            .path("/markets/0-65535/estimated-buy-amount/1?atoms=true&hops=2")
            .filter(&estimated_buy_amount_filter())
            .now_or_never()
            .unwrap()
            .unwrap();
        assert_eq!(token_pair.buy, 0);
        assert_eq!(token_pair.sell, 65535);
        assert_eq!(volume, 1.0);
        assert_eq!(query.atoms, true);
        assert_eq!(query.hops, Some(2));
    }

    #[test]
    fn markets_ok() {
        let (market, query) = warp::test::request()
            .path("/markets/1-2?atoms=true&hops=3")
            .filter(&markets_filter())
            .now_or_never()
            .unwrap()
            .unwrap();
        assert_eq!(market.base, 1);
        assert_eq!(market.quote, 2);
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

    #[test]
    fn estimated_amounts_at_price_ok() {
        let (token_pair, volume, query) = warp::test::request()
            .path("/markets/0-65535/estimated-amounts-at-price/0.5?atoms=true")
            .filter(&estimated_amounts_at_price_filter())
            .now_or_never()
            .unwrap()
            .unwrap();
        assert_eq!(token_pair.buy, 0);
        assert_eq!(token_pair.sell, 65535);
        assert!((volume - 0.5).abs() < f64::EPSILON);
        assert_eq!(query.atoms, true);
        assert_eq!(query.hops, None);
    }

    #[test]
    fn estimated_best_ask_xrate_ok() {
        let (token_pair, query) = warp::test::request()
            .path("/markets/0-65535/estimated-best-ask-price?atoms=true")
            .filter(&estimated_best_ask_price_filter())
            .now_or_never()
            .unwrap()
            .unwrap();
        assert_eq!(token_pair.buy, 0);
        assert_eq!(token_pair.sell, 65535);
        assert_eq!(query.atoms, true);
        assert_eq!(query.hops, None);
    }
}
