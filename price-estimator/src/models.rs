use serde::{Deserialize, Serialize};
use serde_with::rust::display_fromstr;
use std::{cmp::Ordering, convert::From, num::ParseIntError, str::FromStr};

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

impl FromStr for TokenPair {
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

/// The query part of the url.
#[derive(Debug, Deserialize)]
pub struct QueryParameters {
    pub atoms: bool,
    pub hops: Option<u16>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EstimatedOrderResult {
    #[serde(with = "display_fromstr")]
    pub base_token_id: u16,
    #[serde(with = "display_fromstr")]
    pub quote_token_id: u16,
    #[serde(with = "display_fromstr")]
    pub buy_amount_in_base: u128,
    #[serde(with = "display_fromstr")]
    pub sell_amount_in_quote: u128,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct TransitiveOrder {
    pub price: f64,
    pub volume: f64,
}

#[derive(Debug, Serialize)]
pub struct MarketsResult {
    pub asks: Vec<TransitiveOrder>,
    pub bids: Vec<TransitiveOrder>,
}

enum TransitiveOrderbookOrdering {
    PriceAscending,
    PriceDescending,
}

fn sort_and_aggregate_orders_by_price(
    mut orders: Vec<TransitiveOrder>,
    ordering: TransitiveOrderbookOrdering,
) -> Vec<TransitiveOrder> {
    let compare = |a: &TransitiveOrder, b: &TransitiveOrder| {
        let (a, b) = match ordering {
            TransitiveOrderbookOrdering::PriceAscending => (a, b),
            TransitiveOrderbookOrdering::PriceDescending => (b, a),
        };
        a.price.partial_cmp(&b.price).unwrap_or(Ordering::Less)
    };
    orders.sort_unstable_by(compare);
    let mut result = Vec::<TransitiveOrder>::with_capacity(orders.len());
    for order in orders.into_iter().filter(|order| !order.price.is_nan()) {
        match result.last_mut() {
            #[allow(clippy::float_cmp)]
            Some(last) if last.price == order.price => last.volume += order.volume,
            _ => result.push(order),
        }
    }
    result
}

impl From<&pricegraph::TransitiveOrderbook> for MarketsResult {
    fn from(orderbook: &pricegraph::TransitiveOrderbook) -> Self {
        let to_order = |(price, volume)| TransitiveOrder { price, volume };
        // The frontend currently (2020-30-06) requires a specific ordering of the orders. We
        // consider this a bug but until it is fixed we need this workaround.
        let asks = sort_and_aggregate_orders_by_price(
            orderbook.ask_prices().map(to_order).collect(),
            TransitiveOrderbookOrdering::PriceAscending,
        );
        let bids = sort_and_aggregate_orders_by_price(
            orderbook.bid_prices().map(to_order).collect(),
            TransitiveOrderbookOrdering::PriceDescending,
        );
        Self { asks, bids }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    #[test]
    fn estimated_buy_amount_serialization() {
        let original = EstimatedOrderResult {
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

    #[test]
    fn transitive_orderbook_serialization() {
        let original = MarketsResult {
            asks: vec![TransitiveOrder {
                price: 1.0,
                volume: 2.0,
            }],
            bids: vec![TransitiveOrder {
                price: 3.5,
                volume: 4.0,
            }],
        };
        let serialized = serde_json::to_string(&original).unwrap();
        let json: Value = serde_json::from_str(&serialized).unwrap();
        let expected = serde_json::json!({
            "asks": [{"price": 1.0, "volume": 2.0}],
            "bids": [{"price": 3.5, "volume": 4.0}],
        });
        assert_eq!(json, expected);
    }

    #[test]
    fn sanitize_orders_sums_volume_at_same_price() {
        let original = vec![
            TransitiveOrder {
                price: 1.0,
                volume: 1.0,
            },
            TransitiveOrder {
                price: 1.0,
                volume: 2.0,
            },
            TransitiveOrder {
                price: 2.0,
                volume: 3.0,
            },
            TransitiveOrder {
                price: 1.0,
                volume: 4.0,
            },
            TransitiveOrder {
                price: 2.0,
                volume: 5.0,
            },
            TransitiveOrder {
                price: 3.0,
                volume: 6.0,
            },
            TransitiveOrder {
                price: 2.0,
                volume: 7.0,
            },
        ];
        let mut expected = vec![
            TransitiveOrder {
                price: 1.0,
                volume: 7.0,
            },
            TransitiveOrder {
                price: 2.0,
                volume: 15.0,
            },
            TransitiveOrder {
                price: 3.0,
                volume: 6.0,
            },
        ];
        assert_eq!(
            sort_and_aggregate_orders_by_price(
                original.clone(),
                TransitiveOrderbookOrdering::PriceAscending
            ),
            expected
        );
        expected.reverse();
        assert_eq!(
            sort_and_aggregate_orders_by_price(
                original,
                TransitiveOrderbookOrdering::PriceDescending
            ),
            expected
        );
    }
}
