use crate::filter::TokenPair;
use pricegraph::Orderbook;

pub fn estimate_buy_amount(
    token_pair: TokenPair,
    sell_amount_in_quote: f64,
    price_rounding_buffer: f64,
    orderbook: Orderbook,
) -> Option<f64> {
    estimate_price(token_pair, sell_amount_in_quote, orderbook)
        .map(|price| (1.0 - price_rounding_buffer) * price * sell_amount_in_quote)
}

fn estimate_price(
    token_pair: TokenPair,
    sell_amount_in_quote: f64,
    mut orderbook: Orderbook,
) -> Option<f64> {
    orderbook.fill_market_order(token_pair.into(), sell_amount_in_quote)
}
