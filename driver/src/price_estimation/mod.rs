//! Module responsible for aggregating price estimates from various sources to
//! give good price estimates to the solver for better results.

#![allow(dead_code)]

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
    fn get_price(&self, price: f64) -> u128 {
        (price * 10.0f64.powi(self.decimals as _)) as _
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

trait PriceSource {
    fn get_prices(&mut self, tokens: &[Token]) -> Result<HashMap<TokenId, u128>>;
}

macro_rules! token_proxies {
    (const $n:ident = { $( $token:ident => $( $proxy:ident ),* ;)* }) => {
        lazy_static! {
            static ref TOKEN_PROXIES: HashMap<String, HashSet<String>> = {
                let mut proxies = HashMap::new();
                $(
                    proxies.insert(stringify!($token).into(), {
                        let mut tokens = HashSet::new();
                        $(
                            tokens.insert(stringify!($proxy).into());
                        )*
                        tokens
                    });
                )*
                proxies
            };
        }
    };
}

token_proxies!(
    const TOKEN_PROXIES = {
        DAI => USD;
        PAX => USD;
        WETH => ETH;
    }
);
