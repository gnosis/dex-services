mod currency_pair;
mod query;

pub use self::{currency_pair::*, query::*};
use core::token_info::TokenBaseInfo;
use serde::Serialize;
use serde_with::rust::display_fromstr;
use std::cmp::Ordering;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EstimatedOrderResult {
    pub base_token_id: u16,
    pub quote_token_id: u16,
    pub buy_amount_in_base: Amount,
    pub sell_amount_in_quote: Amount,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct TransitiveOrder {
    pub price: f64,
    pub volume: f64,
}

/// Type used for modeling token amounts in either fractional base units or
/// whole atoms.
#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
#[serde(untagged)]
pub enum Amount {
    Atoms(#[serde(with = "display_fromstr")] u128),
    BaseUnits(#[serde(with = "display_fromstr")] f64),
}

impl Amount {
    /// Converts an amount into base units for the specified token.
    pub fn into_base_units(self, token: &TokenBaseInfo) -> Self {
        match self {
            Amount::Atoms(atoms) => {
                Amount::BaseUnits(atoms as f64 / token.base_unit_in_atoms().get() as f64)
            }
            base_units => base_units,
        }
    }

    /// Converts an amount into atoms for the specified token.
    pub fn into_atoms(self, token: &TokenBaseInfo) -> Self {
        match self {
            Amount::BaseUnits(units) => {
                Amount::Atoms((units * token.base_unit_in_atoms().get() as f64) as _)
            }
            atoms => atoms,
        }
    }

    /// Returns the amount in atoms.
    pub fn as_atoms(self, token: &TokenBaseInfo) -> u128 {
        match self.into_atoms(token) {
            Amount::Atoms(atoms) => atoms,
            _ => unreachable!("amount converted into atoms"),
        }
    }
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

/// A type representing a market price estimate result. Prices in a market are
/// always represented in the quote token.
#[derive(Debug, Serialize)]
#[serde(transparent)]
pub struct PriceEstimateResult(pub Option<f64>);

#[derive(Debug, Serialize)]
pub struct ErrorResult {
    pub message: &'static str,
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
            buy_amount_in_base: Amount::Atoms(3),
            sell_amount_in_quote: Amount::BaseUnits(4.2),
        };
        let serialized = serde_json::to_string(&original).unwrap();
        let json: Value = serde_json::from_str(&serialized).unwrap();
        let expected = serde_json::json!({
            "baseTokenId": 1,
            "quoteTokenId": 2,
            "buyAmountInBase": "3",
            "sellAmountInQuote": "4.2",
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

    #[test]
    fn amount_unit_conversion() {
        let owl = TokenBaseInfo {
            alias: "OWL".into(),
            decimals: 18,
        };
        let usdc = TokenBaseInfo {
            alias: "USDC".into(),
            decimals: 6,
        };

        let amount = Amount::BaseUnits(4.2);

        assert_eq!(
            amount.into_atoms(&owl),
            Amount::Atoms(4_200_000_000_000_000_000)
        );
        assert_eq!(amount.into_atoms(&usdc), Amount::Atoms(4_200_000));

        assert_eq!(amount.into_atoms(&owl).into_base_units(&owl), amount);
        assert_eq!(amount.into_atoms(&usdc).into_base_units(&usdc), amount);
    }
}
