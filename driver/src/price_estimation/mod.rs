//! Module responsible for aggregating price estimates from various sources to
//! give good price estimates to the solver for better results.

#![allow(dead_code)]

mod kraken;

use anyhow::Result;
use ethcontract::Address;
use std::collections::HashMap;

/// An opaque token ID.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
struct TokenId(u16);

/// A token reprensentation.
struct Token {
    id: TokenId,
    address: Address,
    symbol: String,
    decimals: u8,
}

impl Token {
    /// Converts the prices from USD into the unit expected by the contract.
    /// This price is relative to the OWL token which is considered pegged at
    /// exactly 1 USD with 18 decimals.
    fn get_price(&self, usd_price: f64) -> u128 {
        let pow = 36 - (self.decimals as i32);
        (usd_price * 10.0f64.powi(pow)) as _
    }
}

/// A price oracle to retrieve price estimates for exchange tokens to help the
/// solver find better solutions.
pub struct PriceOracle {}

/// An abstraction around a type that retrieves price estimate from a source
/// such as an exchange.
#[cfg_attr(test, mockall::automock)]
trait PriceSource {
    fn get_prices(&self, tokens: &[Token]) -> Result<HashMap<TokenId, u128>>;
}
