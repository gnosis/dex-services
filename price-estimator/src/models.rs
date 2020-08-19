mod currency_pair;
mod markets_results;
mod query;

pub use self::{currency_pair::*, markets_results::*, query::*};
use core::token_info::TokenBaseInfo;
use serde::Serialize;
use serde_with::rust::display_fromstr;

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

/// A type representing a market price estimate result. Prices in a market are
/// always represented in the quote token.
#[derive(Debug, Serialize)]
#[serde(transparent)]
pub struct PriceEstimateResult(pub Option<f64>);

impl PriceEstimateResult {
    pub fn into_base_units(
        self,
        base_token_info: &TokenBaseInfo,
        quote_token_info: &TokenBaseInfo,
    ) -> Self {
        Self(self.0.map(|p| {
            p * 10f64.powi(quote_token_info.decimals as i32 - base_token_info.decimals as i32)
                as f64
        }))
    }
}

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

    #[test]
    fn price_estimate_into_base_units() {
        let owl = TokenBaseInfo {
            alias: "OWL".into(),
            decimals: 18,
        };
        let usdc = TokenBaseInfo {
            alias: "USDC".into(),
            decimals: 6,
        };

        let one_owl = 10_u64.pow(18);
        let one_usdc = 10_u64.pow(6);

        let price_estimate = PriceEstimateResult(Some(one_owl as f64 / one_usdc as f64));
        assert_eq!(
            price_estimate.into_base_units(&owl, &usdc).0.unwrap(),
            1.0f64
        )
    }
}
