//! Module responsible for aggregating price estimates from various sources to
//! give good price estimates to the solver for better results.

#[allow(dead_code)]
mod kraken;
#[allow(dead_code)]
mod tokens;

pub use crate::price_finding::TokenId;
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
    symbol: String,
    decimals: u8,
}

impl Token {
    /// Converts the prices from USD into the unit expected by the contract.
    /// This price is relative to the OWL token which is considered pegged at
    /// exactly 1 USD with 18 decimals.
    fn get_owl_price(&self, usd_price: f64) -> u128 {
        let pow = 36 - (self.decimals as i32);
        (usd_price * 10f64.powi(pow)) as _
    }

    /// Creates a new token with a fictional address for testing.
    #[cfg(test)]
    fn test(index: u16, symbol: &str, decimals: u8) -> Token {
        Token {
            id: TokenId(index),
            address: H160::repeat_byte(index as _),
            symbol: symbol.into(),
            decimals,
        }
    }
}

/// A reader for retrieving ERC20 token information from the block-chain.
#[cfg_attr(test, mockall::automock)]
trait TokenReading {
    /// Reads a token given its index in the exchange contract.
    fn read_token(&self, index: u16) -> Result<Token>;
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
            (Token::test(4, "USDC", 6), 0.99, 0.99 * 10f64.powi(30)),
            (Token::test(7, "DAI", 18), 1.01, 1.01 * 10f64.powi(18)),
            (Token::test(42, "FAKE", 32), 1.0, 10f64.powi(4)),
            (Token::test(99, "SCAM", 42), 10f64.powi(10), 10f64.powi(4)),
        ] {
            let owl_price = token.get_owl_price(*usd_price);
            assert_eq!(owl_price, *expected as u128);
        }
    }
}
