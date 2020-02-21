//! Module responsible for aggregating price estimates from various sources to
//! give good price estimates to the solver for better results.

#[allow(dead_code)]
mod kraken;

use crate::models::{TokenId, TokenInfo};
use anyhow::Result;
use ethcontract::H160;
use std::collections::HashMap;

/// A token reprensentation.
///
/// This is a duplicate definition of `TokenInfo` and will be removed once #556
/// is merged.
#[cfg_attr(test, derive(Eq, PartialEq))]
#[derive(Clone, Debug)]
struct Token {
    id: TokenId,
    address: H160,
    info: TokenInfo,
}

impl Token {
    /// Gets the ERC20 token symbol.
    fn symbol(&self) -> &str {
        &self.info.alias
    }

    /// Converts the prices from USD into the unit expected by the contract.
    /// This price is relative to the OWL token which is considered pegged at
    /// exactly 1 USD with 18 decimals.
    fn get_owl_price(&self, usd_price: f64) -> u128 {
        let pow = 36 - (self.info.decimals as i32);
        (usd_price * 10f64.powi(pow)) as _
    }

    /// Creates a new token from its parameters.
    #[cfg(test)]
    pub fn new(id: impl Into<TokenId>, symbol: impl Into<String>, decimals: u8) -> Self {
        Token {
            id: id.into(),
            address: H160::repeat_byte(index as _),
            info: TokenInfo {
                alias: symbol.into(),
                decimals,
                external_price: 0,
            },
        }
    }
}

/// An abstraction around a type that retrieves price estimate from a source
/// such as an exchange.
#[cfg_attr(test, mockall::automock)]
trait PriceSource {
    /// Retrieve current prices relative to the OWL token for the specified
    /// tokens. The OWL token is peged at 1 USD with 18 decimals. Returns a
    /// sparce price array as being unable to find a price is not considered an
    /// error.
    fn get_prices(&self, tokens: &[Token]) -> Result<HashMap<TokenId, u128>>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn token_get_price() {
        for (token, usd_price, expected) in &[
            (Token::new(4, "USDC", 6), 0.99, 0.99 * 10f64.powi(30)),
            (Token::new(7, "DAI", 18), 1.01, 1.01 * 10f64.powi(18)),
            (Token::new(42, "FAKE", 32), 1.0, 10f64.powi(4)),
            (Token::new(99, "SCAM", 42), 10f64.powi(10), 10f64.powi(4)),
        ] {
            let owl_price = token.get_owl_price(*usd_price);
            assert_eq!(owl_price, *expected as u128);
        }
    }
}
