use crate::{
    amounts_at_price, error::RejectionReason, metrics::Metrics, models::*, orderbook::Orderbook,
};
use pricegraph::{Market, Pricegraph, TokenPair, TransitiveOrder};
use services_core::{
    economic_viability::EconomicViabilityComputing,
    models::TokenId,
    token_info::{TokenBaseInfo, TokenInfoFetching},
};
use std::{convert::Infallible, sync::Arc, time::Instant};
use warp::{http::StatusCode, reply::Json, Filter, Rejection, Reply};

/// Handles all supported requests under a `/api/v1` root path.
pub fn all(
    orderbook: Arc<Orderbook>,
    token_info: Arc<dyn TokenInfoFetching>,
    metrics: Arc<Metrics>,
    economic_viability: Arc<dyn EconomicViabilityComputing>,
) -> impl Filter<Extract = impl Reply, Error = Infallible> + Clone + Send {
    let markets = markets(orderbook.clone(), token_info.clone());
    let estimated_buy_amount = estimated_buy_amount(orderbook.clone(), token_info.clone());
    let estimated_amounts_at_price =
        estimated_amounts_at_price(orderbook.clone(), token_info.clone());
    let estimated_best_ask_price = estimated_best_ask_price(orderbook, token_info);
    let minimum_order_size_owl = minimum_order_size_owl(economic_viability);

    let label = |label: &'static str| warp::any().map(move || label);
    let routes_with_labels = warp::path!("api" / "v1" / ..).and(
        (label("markets").and(markets))
            .or(label("estimated_buy_amount").and(estimated_buy_amount))
            .unify()
            .or(label("estimated-amounts-at-price").and(estimated_amounts_at_price))
            .unify()
            .or(label("estimated-best-ask-price").and(estimated_best_ask_price))
            .unify()
            .or(label("minimum-order-size-owl").and(minimum_order_size_owl))
            .unify(),
    );

    let start_time = warp::any().map(Instant::now);
    let handle_metrics = move |start, route, reply| {
        metrics.handle_successful_response(route, start);
        (reply,)
    };

    start_time
        .and(routes_with_labels)
        .map(handle_metrics)
        .recover(handle_rejection)
}

async fn handle_rejection(err: Rejection) -> Result<impl Reply, Infallible> {
    let (code, message) = if let Some(reason) = err.find::<RejectionReason>() {
        log::warn!("rejection reason: {:?}", reason);
        reason.as_http_error()
    } else if err.is_not_found() {
        (StatusCode::NOT_FOUND, "invalid url path")
    } else if let Some(warp::reject::InvalidQuery { .. }) = err.find() {
        (StatusCode::BAD_REQUEST, "invalid url query")
    } else {
        log::warn!("unhandled rejection: {:?}", err);
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            "unexpected internal error",
        )
    };

    let json = warp::reply::json(&ErrorResult { message });
    Ok(warp::reply::with_status(json, code))
}

/// Validate a request of the form
/// `/minimum-order-size-owl` and answer it.
fn minimum_order_size_owl(
    economic_viability: Arc<dyn EconomicViabilityComputing>,
) -> impl Filter<Extract = (Json,), Error = Rejection> + Clone {
    warp::path!("minimum-order-size-owl")
        .and(warp::any().map(move || economic_viability.clone()))
        .and_then(get_minimum_order_size_owl)
}

async fn get_minimum_order_size_owl(
    economic_viability: Arc<dyn EconomicViabilityComputing>,
) -> Result<Json, Rejection> {
    let fee_ratio = 1000;
    // Multiply by 2 because economic viability returns earned fee while we want generated fee.
    let result = economic_viability
        .min_average_fee()
        .await
        .map_err(RejectionReason::InternalError)?
        * 2
        * fee_ratio;
    Result::<Json, Rejection>::Ok(warp::reply::json(&result))
}

/// Validate a request of the form
/// `/markets/<baseTokenId>-<quoteTokenId>`
/// and answer it.
fn markets(
    orderbook: Arc<Orderbook>,
    token_info: Arc<dyn TokenInfoFetching>,
) -> impl Filter<Extract = (Json,), Error = Rejection> + Clone {
    markets_filter()
        .and(warp::get())
        .and(warp::any().map(move || orderbook.clone()))
        .and(warp::any().map(move || token_info.clone()))
        .and_then(get_markets)
}

/// Validate a request of the form
/// `/markets/<baseTokenId>-<quoteTokenId>/estimated-buy-amount/<sellAmountInQuoteToken>`
/// and answer it.
fn estimated_buy_amount(
    orderbook: Arc<Orderbook>,
    token_info: Arc<dyn TokenInfoFetching>,
) -> impl Filter<Extract = (Json,), Error = Rejection> + Clone {
    estimated_buy_amount_filter()
        .and(warp::any().map(move || orderbook.clone()))
        .and(warp::any().map(move || token_info.clone()))
        .and_then(estimate_buy_amount)
}

/// Validate a request of the form:
/// `/markets/<baseTokenId>-<quoteTokenId>/estimated-amounts-at-price/<exchangeRate>`
/// and answer it.
fn estimated_amounts_at_price(
    orderbook: Arc<Orderbook>,
    token_info: Arc<dyn TokenInfoFetching>,
) -> impl Filter<Extract = (Json,), Error = Rejection> + Clone {
    estimated_amounts_at_price_filter()
        .and(warp::any().map(move || orderbook.clone()))
        .and(warp::any().map(move || token_info.clone()))
        .and_then(estimate_amounts_at_price)
}

/// Validate a request of the form:
/// `/markets/<baseTokenId>-<quoteTokenId>/estimated-amounts-at-price/<exchangeRate>`
/// and answer it.
fn estimated_best_ask_price(
    orderbook: Arc<Orderbook>,
    token_infos: Arc<dyn TokenInfoFetching>,
) -> impl Filter<Extract = (Json,), Error = Rejection> + Clone {
    estimated_best_ask_price_filter()
        .and(warp::any().map(move || orderbook.clone()))
        .and(warp::any().map(move || token_infos.clone()))
        .and_then(estimate_best_ask_price)
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
        .map_err(|_| RejectionReason::NoTokenInfo.into())
}

async fn get_market(
    pair: CurrencyPair,
    token_info_fetching: &dyn TokenInfoFetching,
) -> Result<Market, Rejection> {
    pair.as_market(token_info_fetching)
        .await
        .map_err(|_| RejectionReason::TokenNotFound.into())
}

async fn get_markets(
    pair: CurrencyPair,
    query: QueryParameters,
    orderbook: Arc<Orderbook>,
    token_infos: Arc<dyn TokenInfoFetching>,
) -> Result<Json, Rejection> {
    let market = get_market(pair, &*token_infos).await?;
    // This route intentionally uses the raw pricegraph without rounding buffer so that orders are
    // unmodified.
    let transitive_orderbook = orderbook
        .pricegraph(
            query.time,
            &query.ignore_addresses,
            RoundingBuffer::Disabled,
        )
        .await
        .map_err(RejectionReason::InternalError)?
        .transitive_orderbook(market, None);
    let result = MarketsResult::from(&transitive_orderbook);
    let result = match query.unit {
        Unit::Atoms => result,
        Unit::BaseUnits => {
            let base_token_info = get_token_info(market.base, token_infos.as_ref()).await?;
            let quote_token_info = get_token_info(market.quote, token_infos.as_ref()).await?;
            result.into_base_units(&base_token_info, &quote_token_info)
        }
    };
    Ok(warp::reply::json(&result))
}

async fn estimate_buy_amount(
    pair: CurrencyPair,
    sell_amount_in_quote: f64,
    query: QueryParameters,
    orderbook: Arc<Orderbook>,
    token_infos: Arc<dyn TokenInfoFetching>,
) -> Result<Json, Rejection> {
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
    let pricegraph = orderbook
        .pricegraph(query.time, &query.ignore_addresses, query.rounding_buffer)
        .await
        .map_err(RejectionReason::InternalError)?;
    // This reduced sell amount is what the solver would see after applying the rounding buffer.
    let sell_amount_in_quote_atoms = match query.rounding_buffer {
        RoundingBuffer::Enabled => f64::max(
            sell_amount_in_quote_atoms - orderbook.rounding_buffer(token_pair).await,
            0.0,
        ),
        RoundingBuffer::Disabled => sell_amount_in_quote_atoms,
    };
    let transitive_order = pricegraph.order_for_sell_amount(token_pair, sell_amount_in_quote_atoms);

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
) -> Result<Json, Rejection> {
    let token_pair = get_market(pair, &*token_infos).await?.bid_pair();
    let pricegraph = orderbook
        .pricegraph(query.time, &query.ignore_addresses, query.rounding_buffer)
        .await
        .map_err(RejectionReason::InternalError)?;
    let rounding_buffer = match query.rounding_buffer {
        RoundingBuffer::Enabled => Some(orderbook.rounding_buffer(token_pair).await),
        RoundingBuffer::Disabled => None,
    };
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
    rounding_buffer: Option<f64>,
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
) -> Result<Json, Rejection> {
    let market = get_market(pair, &*token_infos).await?;
    let price = orderbook
        .pricegraph(query.time, &query.ignore_addresses, query.rounding_buffer)
        .await
        .map_err(RejectionReason::InternalError)?
        .best_ask_transitive_order(market)
        .map(|order| order.overlapping_exchange_rate().recip());

    let result = PriceEstimateResult(price);
    let result = match query.unit {
        Unit::Atoms => result,
        Unit::BaseUnits => {
            let base_token_info = get_token_info(market.base, token_infos.as_ref()).await?;
            let quote_token_info = get_token_info(market.quote, token_infos.as_ref()).await?;
            PriceEstimateResult(price).into_base_units(&base_token_info, &quote_token_info)
        }
    };
    Ok(warp::reply::json(&result))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::infallible_price_source::PriceCacheUpdater;
    use anyhow::{anyhow, Result};
    use futures::future::FutureExt as _;
    use services_core::{
        economic_viability::FixedEconomicViabilityComputer, orderbook::NoopOrderbook,
    };

    fn empty_token_info() -> impl TokenInfoFetching {
        struct TokenInfoFetcher {}
        #[async_trait::async_trait]
        impl TokenInfoFetching for TokenInfoFetcher {
            async fn get_token_info(
                &self,
                _: TokenId,
            ) -> Result<services_core::token_info::TokenBaseInfo> {
                Err(anyhow!(""))
            }
            async fn all_ids(&self) -> Result<Vec<TokenId>> {
                Ok(Default::default())
            }
        }
        TokenInfoFetcher {}
    }

    fn all_filter() -> impl Filter<Extract = impl Reply, Error = Infallible> + Clone {
        let token_info = Arc::new(empty_token_info());
        let orderbook = Arc::new(Orderbook::new(
            Box::new(NoopOrderbook),
            PriceCacheUpdater::new(token_info.clone(), Vec::new()),
            1.0,
            TokenId(1),
        ));
        let metrics = Arc::new(Metrics::new(&prometheus::Registry::new()).unwrap());
        let economic_viability = Arc::new(FixedEconomicViabilityComputer::new(0, 0.into()));
        all(orderbook, token_info, metrics, economic_viability)
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
