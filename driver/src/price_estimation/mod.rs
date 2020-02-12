//! Module responsible for aggregating price estimates from various sources to
//! give good price estimates to the solver for better results.

#![allow(dead_code)]

#[macro_use]
mod macros;

mod kraken;

use anyhow::Result;
use ethcontract::Address;
use lazy_static::lazy_static;
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;

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
///
/// The price retrieval is done on a separate thread to ensure that even if the
/// retrieval takes longer than expected, it does not take time from the solver
/// for finding a solution.
pub struct PriceOracle {
    prices: Arc<Mutex<HashMap<TokenId, u128>>>,
    update: JoinHandle<()>,
}

/// An abstraction around a type that retrieves price estimate from a source
/// such as an exchange.
#[cfg_attr(test, mockall::automock)]
trait PriceSource {
    fn get_prices(&self, tokens: &[Token]) -> Result<HashMap<TokenId, u128>>;
}

token_proxies!(
    const TOKEN_PROXIES = {
        <=> DAI, GUSD, PAX, TUSD, USD, USDC, USDT, sUSD;
        WETH, sETH => ETH;
    }
);
