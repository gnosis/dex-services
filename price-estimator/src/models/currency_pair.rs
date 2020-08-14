//! Module containing currency pair model implementation.

use anyhow::{anyhow, bail, Error, Result};
use core::token_info::TokenInfoFetching;
use ethcontract::Address;
use pricegraph::{Market, TokenId};
use std::str::FromStr;

/// A currency pair of two exchange tokens.
#[derive(Clone, Debug, PartialEq)]
pub struct CurrencyPair {
    pub base: TokenRef,
    pub quote: TokenRef,
}

impl CurrencyPair {
    /// Convert the token pair into a market for `pricegraph` consumption.
    pub async fn as_market(&self, token_infos: &dyn TokenInfoFetching) -> Result<Market> {
        let (base, quote) = futures::try_join!(
            self.base.as_token_id(token_infos),
            self.quote.as_token_id(token_infos),
        )?;

        Ok(Market { base, quote })
    }
}

impl FromStr for CurrencyPair {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self> {
        let (base, quote) = {
            let mut parts = s.split('-');
            let pair = parts
                .next()
                .and_then(|base| parts.next().map(|quote| (base, quote)));
            if parts.next().is_none() {
                pair
            } else {
                None
            }
        }
        .ok_or_else(|| anyhow!("currency pair expected 'X-Y' format"))?;

        Ok(CurrencyPair {
            base: base.parse()?,
            quote: quote.parse()?,
        })
    }
}

/// A token reference, either by token ID, address, or symbol.
#[derive(Clone, Debug, PartialEq)]
pub enum TokenRef {
    Id(TokenId),
    Address(Address),
    Symbol(String),
}

impl TokenRef {
    /// Convert this token reference into an exchange token ID.
    pub async fn as_token_id(&self, token_infos: &dyn TokenInfoFetching) -> Result<TokenId> {
        match self {
            TokenRef::Id(id) => Ok(*id),
            TokenRef::Address(_) => bail!("not yet implemented"),
            TokenRef::Symbol(symbol) => {
                let (id, _) = token_infos
                    .find_token_by_symbol(symbol)
                    .await?
                    .ok_or_else(|| anyhow!("token symbol {} not found", symbol))?;
                Ok(id.into())
            }
        }
    }
}

impl FromStr for TokenRef {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self> {
        // NOTE: Make sure to try parsing by ID and address before falling back
        // to the symbol variant, since it can technically accomodate all values
        // of `s`.

        if let Ok(id) = s.parse() {
            Ok(TokenRef::Id(id))
        } else if let Some(address) = parse_address(s) {
            Ok(TokenRef::Address(address))
        } else {
            Ok(TokenRef::Symbol(s.to_owned()))
        }
    }
}

/// Parses an address in the `0x...` format.
fn parse_address(s: &str) -> Option<Address> {
    s.strip_prefix("0x")?.parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_currency_pair() {
        let pair = "42-1337".parse::<CurrencyPair>().unwrap();
        assert_eq!(
            pair,
            CurrencyPair {
                base: TokenRef::Id(42),
                quote: TokenRef::Id(1337),
            }
        );
    }

    #[test]
    fn token_symbol_and_address() {
        let pair = "WETH-0x1A5F9352Af8aF974bFC03399e3767DF6370d82e4"
            .parse::<CurrencyPair>()
            .unwrap();
        assert_eq!(
            pair,
            CurrencyPair {
                base: TokenRef::Symbol("WETH".into()),
                quote: TokenRef::Address(
                    "1A5F9352Af8aF974bFC03399e3767DF6370d82e4".parse().unwrap()
                ),
            }
        );
    }
}
