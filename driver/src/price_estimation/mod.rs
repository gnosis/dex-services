//! Module responsible for aggregating price estimates from various sources to
//! give good price estimates to the solver for better results.

#![allow(dead_code)]

mod kraken;

use self::kraken::KrakenClient;
use crate::contracts::erc20::ERC20Detailed;
use crate::contracts::stablex_contract::BatchExchange;
use crate::contracts::Web3;
use crate::util::FutureWaitExt;
use anyhow::Result;
use ethcontract::Address;
use lazy_static::lazy_static;
use log::warn;
use serde::{Serialize, Serializer};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::iter::FromIterator;

/// An opaque token ID.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialOrd, PartialEq)]
pub struct TokenId(u16);

/// A token reprensentation.
struct Token {
    id: TokenId,
    address: Address,
    symbol: String,
    decimals: u8,
}

lazy_static! {
    static ref SYMBOL_OVERRIDES: HashMap<String, String> = hash_map! {
        "WETH" => "ETH".to_owned(),
    };
}

impl Token {
    /// Retrieves the token information from the Ethereum network.
    fn read(web3: &Web3, contract: &BatchExchange, index: u16) -> Result<Token> {
        let address = contract
            .token_id_to_address_map(index as _)
            .call()
            .wait()
            .unwrap();
        let erc20 = ERC20Detailed::at(web3, address);
        let symbol = {
            // NOTE: We check if the symbol is part of the overrides map, and
            //   use the overridden value if it is. This allows ERC20 tokens
            //   like WETH to be treated by ETH, since exchanges generally only
            //   track ETH prices and not WETH.
            let symbol = erc20.symbol().call().wait().unwrap();
            SYMBOL_OVERRIDES
                .get(&symbol)
                .map(|s| s.to_owned())
                .unwrap_or(symbol)
        };
        let decimals = erc20.decimals().call().wait().unwrap() as u8;

        Ok(Token {
            id: TokenId(index),
            address,
            symbol,
            decimals,
        })
    }

    /// Converts the prices from USD into the unit expected by the contract.
    /// This price is relative to the OWL token which is considered pegged at
    /// exactly 1 USD with 18 decimals.
    fn get_price(&self, usd_price: f64) -> u128 {
        let pow = 36 - (self.decimals as i32);
        (usd_price * 10.0f64.powi(pow)) as _
    }
}

impl Serialize for TokenId {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        format!("T{:04}", self.0).serialize(serializer)
    }
}

/// A price oracle to retrieve price estimates for exchange tokens to help the
/// solver find better solutions.
pub struct PriceOracle {
    /// The web3 provider used by the price oracle.
    web3: Web3,
    /// The exchange contract.
    contract: BatchExchange,
    /// Cache of token information that is used for
    tokens: Vec<Token>,
    /// Cached count of read tokens.
    read_token_count: u16,
    /// The price source being used.
    price_source: Box<dyn PriceSource>,
}

lazy_static! {
    /// Tokens that are pegged to USD.
    static ref USD_TOKENS: HashSet<String> = HashSet::from_iter(
        ["USDT", "TUSD", "USDC", "PAX", "GUSD", "DAI", "sUSD"]
            .iter()
            .map(|&s| s.to_owned())
    );
}

impl PriceOracle {
    /// Create a new price oracle that is responsible for estimating prices.
    pub fn new(web3: Web3, contract: BatchExchange) -> Result<Self> {
        let kraken_client = KrakenClient::new()?;
        Ok(PriceOracle::with_price_source(
            web3,
            contract,
            kraken_client,
        ))
    }

    /// Create a new price oracle with the provided price source.
    fn with_price_source(
        web3: Web3,
        contract: BatchExchange,
        price_source: impl PriceSource + 'static,
    ) -> Self {
        PriceOracle {
            web3,
            contract,
            tokens: Vec::new(),
            // NOTE: We start with `read_token_count` as `1` since the first
            // token is the fee token, which has a fixed price so we do not
            // try to get price estimate for it.
            read_token_count: 1,
            price_source: Box::new(price_source),
        }
    }

    /// Update the token cache.
    fn update_tokens(&mut self) {
        let num_tokens = self.contract.num_tokens().call().wait().unwrap_or(1) as u16;

        for index in self.read_token_count..num_tokens {
            let token = match Token::read(&self.web3, &self.contract, index) {
                Ok(token) => token,
                Err(err) => {
                    warn!(
                        "error retrieving token information for token {},\
                         price will not be estimated for this token: {}",
                        index, err,
                    );
                    continue;
                }
            };
            self.tokens.push(token);
        }

        self.read_token_count = num_tokens;
    }

    /// Initialize the price oracle by retrieving token information. This will
    /// speed up the first time prices get retrieved.
    ///
    /// Note this method is implicitely called when retrieving prices and is
    /// just a convenience to prime the token info cache to speed up the first
    /// call.
    pub fn initialize(&mut self) {
        self.update_tokens();
    }

    /// Retrieve price estimates for currently listed exchange tokens.
    ///
    /// Note that a sparse token ID to price map is returned since the price
    /// oracle does not guarantee that it will find a price for each token.
    pub fn get_price_estimates(&mut self) -> PriceEstimates {
        self.update_tokens();

        // TODO(nlordell): aggregate multiple price sources for better prices.
        let mut prices = match self.price_source.get_prices(&self.tokens) {
            Ok(prices) => prices,
            Err(err) => {
                warn!(
                    "error retrieving price estimates, solution results may be sub-optimal: {}",
                    err
                );
                HashMap::new()
            }
        };

        // NOTE: Add price estimates for USD stable coins if they were not found
        //   by the price source.
        for token in self
            .tokens
            .iter()
            .filter(|token| USD_TOKENS.contains(&token.symbol))
        {
            prices
                .entry(token.id)
                .or_insert_with(|| token.get_price(1.0));
        }

        PriceEstimates::new(prices)
    }
}

/// An abstraction around a type that retrieves price estimate from a source
/// such as an exchange.
#[cfg_attr(test, mockall::automock)]
trait PriceSource {
    fn get_prices(&self, tokens: &[Token]) -> Result<HashMap<TokenId, u128>>;
}

/// A price estimate result for the given tokens.
///
/// This type implements JSON deserialization so that it is accepted as input to
/// the solver.
#[derive(Clone, Debug, Default, Serialize)]
#[serde(transparent)]
pub struct PriceEstimates(pub BTreeMap<TokenId, u128>);

impl PriceEstimates {
    /// Create a new price estimate mapping from an iterator of estimates.
    fn new<I>(estimates: I) -> Self
    where
        I: IntoIterator<Item = (TokenId, u128)>,
    {
        PriceEstimates(BTreeMap::from_iter(estimates))
    }
}
