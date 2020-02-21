//! This module implements the ERC20 smart contract interface as well a trait
//! and implementation to read it from the block chain.

use super::Web3;
use crate::models::TokenInfo;
use crate::util::FutureWaitExt;
use anyhow::Result;
use ethcontract::H160;
use lazy_static::lazy_static;

/// Trait for reading ERC20 token information from the block chain.
#[cfg_attr(test, mockall::automock)]
pub trait TokenReading {
    /// Retrieve token information by address.
    fn read_token_info(&self, token_address: H160) -> Result<TokenInfo>;
}

include!(concat!(env!("OUT_DIR"), "/erc20_detailed.rs"));

/// Web3 `TokenReading` implementation.
pub struct TokenReader {
    web3: Web3,
}

impl TokenReader {
    /// Create a new token reader from a web3 instnace.
    #[allow(dead_code)]
    pub fn new(web3: Web3) -> Self {
        TokenReader { web3 }
    }
}

impl TokenReading for TokenReader {
    /// Retrieve token information by address.
    fn read_token_info(&self, token_address: H160) -> Result<TokenInfo> {
        let erc20 = ERC20Detailed::at(&self.web3, token_address);
        let alias = erc20.symbol().call().wait()?;
        let decimals = erc20.decimals().call().wait()?;

        Ok(TokenInfo {
            alias,
            decimals,
            external_price: None,
        })
    }
}
