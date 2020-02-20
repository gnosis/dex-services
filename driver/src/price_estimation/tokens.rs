//! This module implements token reading from the EVM.

use super::{Token, TokenReading};
use crate::contracts::erc20::ERC20Detailed;
use crate::contracts::stablex_contract::BatchExchange;
use crate::price_finding::TokenId;
use crate::util::FutureWaitExt;
use anyhow::Result;
use lazy_static::lazy_static;
use std::collections::HashMap;

lazy_static! {
    static ref SYMBOL_OVERRIDES: HashMap<String, String> = hash_map! {
        "WETH" => "ETH".to_owned(),
    };
}

/// Default token reader implementation.
struct EthTokenReader {
    contract: BatchExchange,
}

impl EthTokenReader {
    /// Creates a new token reader from a web3 provider and exchange contract
    /// instance.
    fn new(contract: BatchExchange) -> Self {
        EthTokenReader {
            contract,
        }
    }
}

impl TokenReading for EthTokenReader {
    /// Retrieves the token information from the Ethereum network.
    fn read_token(&self, index: u16) -> Result<Token> {
        let address = self
            .contract
            .token_id_to_address_map(index as _)
            .call()
            .wait()?;
        let erc20 = ERC20Detailed::at(&self.contract.web3(), address);
        let symbol = {
            // NOTE: We check if the symbol is part of the overrides map, and
            //   use the overridden value if it is. This allows ERC20 tokens
            //   like WETH to be treated by ETH, since exchanges generally only
            //   track ETH prices and not WETH.
            let symbol = erc20.symbol().call().wait()?;
            SYMBOL_OVERRIDES
                .get(&symbol)
                .map(|s| s.to_owned())
                .unwrap_or(symbol)
        };
        let decimals = erc20.decimals().call().wait()?;

        Ok(Token {
            id: TokenId(index),
            address,
            symbol,
            decimals,
        })
    }
}
