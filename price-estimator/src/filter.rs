use crate::{
    amounts_at_price,
    error::{self, InternalError},
    models::*,
    orderbook::{Orderbook, PricegraphKind},
};
use core::{
    models::TokenId,
    token_info::{TokenBaseInfo, TokenInfoFetching},
};
use pricegraph::{Market, Pricegraph, TokenPair, TransitiveOrder};
use std::convert::Infallible;
use std::sync::Arc;
use warp::{
    http::StatusCode,
    reject::{self, Reject},
    Filter, Rejection, Reply,
};

/// Handles all supported requests under a `/api/v1` root path.
pub fn all(
    orderbook: Arc<Orderbook>,
    token_info: Arc<dyn TokenInfoFetching>,
) -> impl Filter<Extract = impl Reply, Error = Infallible> + Clone {
    warp::path!("api" / "v1" / ..)
        .and(
            markets(orderbook.clone(), token_info.clone())
                .or(estimated_buy_amount(orderbook.clone(), token_info.clone()))
                .or(estimated_amounts_at_price(
                    orderbook.clone(),
                    token_info.clone(),
                ))
                .or(estimated_best_ask_price(orderbook, token_info)),
        )
        .recover(handle_rejection)
}

#[derive(Debug)]
struct NoTokenInfo;
impl Reject for NoTokenInfo {}

#[derive(Debug)]
struct TokenNotFound;
impl Reject for TokenNotFound {}

async fn handle_rejection(err: Rejection) -> Result<impl Reply, Infallible> {
    let code;
    let message;

    if err.is_not_found() {
        code = StatusCode::NOT_FOUND;
        message = "invalid url path";
    } else if let Some(NoTokenInfo) = err.find() {
        code = StatusCode::BAD_REQUEST;
        message = "request with atoms=true for token we don't have erc20 info for";
    } else if let Some(TokenNotFound) = err.find() {
        code = StatusCode::BAD_REQUEST;
        message = "token symbol or address not found";
    } else if let Some(InternalError(err)) = err.find() {
        log::warn!("internal server error: {:?}", err);
        code = StatusCode::INTERNAL_SERVER_ERROR;
        message = "internal server error";
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
fn markets(
    orderbook: Arc<Orderbook>,
    token_info: Arc<dyn TokenInfoFetching>,
) -> impl Filter<Extract = impl Reply, Error = Rejection> + Clone {
    markets_filter()
        .and(warp::any().map(move || orderbook.clone()))
        .and(warp::any().map(move || token_info.clone()))
        .and_then(get_markets)
        .with(warp::log("price_estimator::api::markets"))
}

/// Validate a request of the form
/// `/markets/<baseTokenId>-<quoteTokenId>/estimated-buy-amount/<sellAmountInQuoteToken>`
/// and answer it.
fn estimated_buy_amount(
    orderbook: Arc<Orderbook>,
    token_info: Arc<dyn TokenInfoFetching>,
) -> impl Filter<Extract = impl Reply, Error = Rejection> + Clone {
    estimated_buy_amount_filter()
        .and(warp::any().map(move || orderbook.clone()))
        .and(warp::any().map(move || token_info.clone()))
        .and_then(estimate_buy_amount)
        .with(warp::log("price_estimator::api::estimate_buy_amount"))
}

/// Validate a request of the form:
/// `/markets/<baseTokenId>-<quoteTokenId>/estimated-amounts-at-price/<exchangeRate>`
/// and answer it.
fn estimated_amounts_at_price(
    orderbook: Arc<Orderbook>,
    token_info: Arc<dyn TokenInfoFetching>,
) -> impl Filter<Extract = impl Reply, Error = Rejection> + Clone {
    estimated_amounts_at_price_filter()
        .and(warp::any().map(move || orderbook.clone()))
        .and(warp::any().map(move || token_info.clone()))
        .and_then(estimate_amounts_at_price)
        .with(warp::log("price_estimator::api::estimate_amounts_at_price"))
}

/// Validate a request of the form:
/// `/markets/<baseTokenId>-<quoteTokenId>/estimated-amounts-at-price/<exchangeRate>`
/// and answer it.
fn estimated_best_ask_price(
    orderbook: Arc<Orderbook>,
    token_infos: Arc<dyn TokenInfoFetching>,
) -> impl Filter<Extract = impl Reply, Error = Rejection> + Clone {
    estimated_best_ask_price_filter()
        .and(warp::any().map(move || orderbook.clone()))
        .and(warp::any().map(move || token_infos.clone()))
        .and_then(estimate_best_ask_price)
        .with(warp::log("price_estimator::api::estimate_best_ask_price"))
}

fn markets_prefix() -> impl Filter<Extract = (CurrencyPair,), Error = Rejection> + Copy {
    warp::path!("markets" / CurrencyPair / ..)
}

fn markets_filter(
) -> impl Filter<Extract = (CurrencyPair, QueryParameters), Error = Rejection> + Copy {
    markets_prefix()
        .and(warp::path::end())
        .and(warp::get())
        .and(warp::query::<QueryParameters>())
}

fn estimated_buy_amount_filter(
) -> impl Filter<Extract = (CurrencyPair, f64, QueryParameters), Error = Rejection> + Copy {
    markets_prefix()
        .and(warp::path!("estimated-buy-amount" / f64))
        .and(warp::get())
        .and(warp::query::<QueryParameters>())
}

fn estimated_amounts_at_price_filter(
) -> impl Filter<Extract = (CurrencyPair, f64, QueryParameters), Error = Rejection> + Copy {
    markets_prefix()
        .and(warp::path!("estimated-amounts-at-price" / f64))
        .and(warp::get())
        .and(warp::query::<QueryParameters>())
}

fn estimated_best_ask_price_filter(
) -> impl Filter<Extract = (CurrencyPair, QueryParameters), Error = Rejection> + Copy {
    markets_prefix()
        .and(warp::path!("estimated-best-ask-price"))
        .and(warp::get())
        .and(warp::query::<QueryParameters>())
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

async fn get_market(
    pair: CurrencyPair,
    token_info_fetching: &dyn TokenInfoFetching,
) -> Result<Market, Rejection> {
    pair.as_market(token_info_fetching)
        .await
        .map_err(|_| reject::custom(TokenNotFound))
}

async fn get_markets(
    pair: CurrencyPair,
    query: QueryParameters,
    orderbook: Arc<Orderbook>,
    token_infos: Arc<dyn TokenInfoFetching>,
) -> Result<impl Reply, Rejection> {
    if query.unit != Unit::Atoms {
        return Err(warp::reject());
    }
    let market = get_market(pair, &*token_infos).await?;
    // This route intentionally uses the raw pricegraph without rounding buffer so that orders are
    // unmodified.
    let transitive_orderbook = orderbook
        .pricegraph(query.time, PricegraphKind::Raw)
        .await
        .map_err(error::internal_server_rejection)?
        .transitive_orderbook(market, None);
    let result = MarketsResult::from(&transitive_orderbook);
    Ok(warp::reply::json(&result))
}

async fn estimate_buy_amount(
    pair: CurrencyPair,
    sell_amount_in_quote: f64,
    query: QueryParameters,
    orderbook: Arc<Orderbook>,
    token_infos: Arc<dyn TokenInfoFetching>,
) -> Result<impl Reply, Rejection> {
    let token_pair = get_market(pair, &*token_infos).await?.bid_pair();
    let (sell_amount_in_quote, sell_amount_in_quote_atoms) = match query.unit {
        Unit::Atoms => (
            Amount::Atoms(sell_amount_in_quote as _),
            sell_amount_in_quote,
        ),
        Unit::BaseUnits => {
            let token_info = get_token_info(token_pair.sell, token_infos.as_ref()).await?;
            let amount = Amount::BaseUnits(sell_amount_in_quote);
            (amount, amount.as_atoms(&token_info) as _)
        }
    };
    let rounding_buffer = orderbook.rounding_buffer(token_pair).await;
    let pricegraph = orderbook
        .pricegraph(query.time, PricegraphKind::WithRoundingBuffer)
        .await
        .map_err(error::internal_server_rejection)?;
    // This reduced sell amount is what the solver would see after applying the rounding buffer.
    let transitive_order = pricegraph.order_for_sell_amount(
        token_pair,
        f64::max(sell_amount_in_quote_atoms - rounding_buffer, 0.0),
    );

    let mut buy_amount_in_base =
        Amount::Atoms(transitive_order.map(|order| order.buy).unwrap_or_default() as _);
    if query.unit == Unit::BaseUnits {
        let token_info = get_token_info(token_pair.buy, token_infos.as_ref()).await?;
        buy_amount_in_base = buy_amount_in_base.into_base_units(&token_info)
    };

    let result = EstimatedOrderResult {
        base_token_id: token_pair.buy,
        quote_token_id: token_pair.sell,
        sell_amount_in_quote,
        buy_amount_in_base,
    };
    Ok(warp::reply::json(&result))
}

async fn estimate_amounts_at_price(
    pair: CurrencyPair,
    price_in_quote: f64,
    query: QueryParameters,
    orderbook: Arc<Orderbook>,
    token_infos: Arc<dyn TokenInfoFetching>,
) -> Result<impl Reply, Rejection> {
    let token_pair = get_market(pair, &*token_infos).await?.bid_pair();
    let pricegraph = orderbook
        .pricegraph(query.time, PricegraphKind::WithRoundingBuffer)
        .await
        .map_err(error::internal_server_rejection)?;
    let rounding_buffer = orderbook.rounding_buffer(token_pair).await;
    let result = match query.unit {
        Unit::Atoms => estimate_amounts_at_price_atoms(
            token_pair,
            price_in_quote,
            &pricegraph,
            rounding_buffer,
        ),
        Unit::BaseUnits => {
            let buy_token_info = get_token_info(token_pair.buy, token_infos.as_ref()).await?;
            let sell_token_info = get_token_info(token_pair.sell, token_infos.as_ref()).await?;
            let price_in_quote_atoms = price_in_quote
                * (sell_token_info.base_unit_in_atoms().get() as f64
                    / buy_token_info.base_unit_in_atoms().get() as f64);
            let mut result = estimate_amounts_at_price_atoms(
                token_pair,
                price_in_quote_atoms,
                &pricegraph,
                rounding_buffer,
            );
            result.buy_amount_in_base = result.buy_amount_in_base.into_base_units(&buy_token_info);
            result.sell_amount_in_quote = result
                .sell_amount_in_quote
                .into_base_units(&sell_token_info);
            result
        }
    };
    Ok(warp::reply::json(&result))
}

/// Like `estimate_amounts_at_price` but the price is given and returned in atoms.
fn estimate_amounts_at_price_atoms(
    token_pair: TokenPair,
    price_in_quote: f64,
    pricegraph: &Pricegraph,
    rounding_buffer: f64,
) -> EstimatedOrderResult {
    // NOTE: The price in quote is `sell_amount / buy_amount` which is the
    // inverse of an exchange rate.
    let limit_price = 1.0 / price_in_quote;
    let order = amounts_at_price::order_at_price_with_rounding_buffer(
        token_pair,
        limit_price,
        pricegraph,
        rounding_buffer,
    )
    .unwrap_or(TransitiveOrder {
        buy: 0.0,
        sell: 0.0,
    });
    EstimatedOrderResult {
        base_token_id: token_pair.buy,
        quote_token_id: token_pair.sell,
        sell_amount_in_quote: Amount::Atoms(order.sell as _),
        buy_amount_in_base: Amount::Atoms(order.buy as _),
    }
}

async fn estimate_best_ask_price(
    pair: CurrencyPair,
    query: QueryParameters,
    orderbook: Arc<Orderbook>,
    token_infos: Arc<dyn TokenInfoFetching>,
) -> Result<impl Reply, Rejection> {
    if query.unit != Unit::Atoms {
        return Err(warp::reject());
    }
    let token_pair = get_market(pair, &*token_infos).await?.bid_pair();
    let price = orderbook
        .pricegraph(query.time, PricegraphKind::WithRoundingBuffer)
        .await
        .map_err(error::internal_server_rejection)?
        .estimate_limit_price(token_pair, 0.0);

    let result = PriceEstimateResult(price);
    Ok(warp::reply::json(&result))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::infallible_price_source::PriceCacheUpdater;
    use anyhow::{anyhow, Result};
    use core::orderbook::NoopOrderbook;
    use futures::future::{BoxFuture, FutureExt as _};

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
        let token_info = Arc::new(empty_token_info());
        let orderbook = Arc::new(Orderbook::new(
            Box::new(NoopOrderbook {}),
            PriceCacheUpdater::new(token_info.clone(), Vec::new()),
            1.0,
        ));
        all(orderbook, token_info)
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
    fn token_by_symbol_and_address() {
        let (pair, _) = warp::test::request()
            .path("/markets/WETH-0x1A5F9352Af8aF974bFC03399e3767DF6370d82e4?atoms=false")
            .filter(&markets_filter())
            .now_or_never()
            .unwrap()
            .unwrap();
        assert_eq!(
            pair,
            CurrencyPair {
                base: TokenRef::Symbol("WETH".into()),
                quote: TokenRef::Address(
                    "1A5F9352Af8aF974bFC03399e3767DF6370d82e4".parse().unwrap(),
                ),
            }
        );
    }

    #[test]
    #[allow(clippy::float_cmp)]
    fn estimated_buy_amount_ok() {
        let (pair, volume, query) = warp::test::request()
            .path("/markets/0-65535/estimated-buy-amount/1?atoms=true&hops=2")
            .filter(&estimated_buy_amount_filter())
            .now_or_never()
            .unwrap()
            .unwrap();
        assert_eq!(pair.base, TokenRef::Id(0));
        assert_eq!(pair.quote, TokenRef::Id(65535));
        assert_eq!(volume, 1.0);
        assert_eq!(query.unit, Unit::Atoms);
        assert_eq!(query.hops, Some(2));
    }

    #[test]
    fn markets_ok() {
        let (pair, query) = warp::test::request()
            .path("/markets/1-2?atoms=true&hops=3&batchId=123")
            .filter(&markets_filter())
            .now_or_never()
            .unwrap()
            .unwrap();
        assert_eq!(pair.base, TokenRef::Id(1));
        assert_eq!(pair.quote, TokenRef::Id(2));
        assert_eq!(query.unit, Unit::Atoms);
        assert_eq!(query.hops, Some(3));
        assert_eq!(query.time, EstimationTime::Batch(123.into()));
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
    fn estimated_amounts_at_price_ok() {
        let (pair, volume, query) = warp::test::request()
            .path("/markets/0-65535/estimated-amounts-at-price/0.5?atoms=true")
            .filter(&estimated_amounts_at_price_filter())
            .now_or_never()
            .unwrap()
            .unwrap();
        assert_eq!(pair.base, TokenRef::Id(0));
        assert_eq!(pair.quote, TokenRef::Id(65535));
        assert!((volume - 0.5).abs() < f64::EPSILON);
        assert_eq!(query.unit, Unit::Atoms);
        assert_eq!(query.hops, None);
    }

    #[test]
    fn estimated_best_ask_xrate_ok() {
        let (pair, query) = warp::test::request()
            .path("/markets/0-65535/estimated-best-ask-price?atoms=true")
            .filter(&estimated_best_ask_price_filter())
            .now_or_never()
            .unwrap()
            .unwrap();
        assert_eq!(pair.base, TokenRef::Id(0));
        assert_eq!(pair.quote, TokenRef::Id(65535));
        assert_eq!(query.unit, Unit::Atoms);
        assert_eq!(query.hops, None);
    }
}
