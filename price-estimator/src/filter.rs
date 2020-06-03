use std::num::ParseIntError;
use warp::Filter;

/// Validate a request of the form
/// `/markets/<baseTokenId>-<quoteTokenId>/estimated-buy-amount/<sellAmountInQuoteToken>`
/// and extract the url parameters.
pub fn estimated_buy_amount(
) -> impl Filter<Extract = (TokenPair, u128), Error = warp::Rejection> + Copy {
    warp::path!("markets" / TokenPair / "estimated-buy-amount" / u128)
}

#[derive(Debug, Copy, Clone)]
pub struct TokenPair {
    buy_token_id: u16,
    sell_token_id: u16,
}

impl std::convert::Into<pricegraph::TokenPair> for TokenPair {
    fn into(self) -> pricegraph::TokenPair {
        pricegraph::TokenPair {
            buy: self.buy_token_id,
            sell: self.sell_token_id,
        }
    }
}

#[derive(thiserror::Error, Debug)]
pub enum ParseTokenPairError {
    #[error("wrong number of tokens")]
    WrongNumberOfTokens,
    #[error("parse int error")]
    ParseIntError(#[from] ParseIntError),
}

impl std::str::FromStr for TokenPair {
    type Err = ParseTokenPairError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let mut split = s.split('-');
        let mut next_token_id = || -> Result<u16, ParseTokenPairError> {
            let token_string = split
                .next()
                .ok_or(ParseTokenPairError::WrongNumberOfTokens)?;
            token_string.parse().map_err(From::from)
        };
        let buy_token_id = next_token_id()?;
        let sell_token_id = next_token_id()?;
        if split.next().is_some() {
            return Err(ParseTokenPairError::WrongNumberOfTokens);
        }
        Ok(Self {
            buy_token_id,
            sell_token_id,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::future::FutureExt as _;

    #[test]
    fn estimated_buy_amount_ok() {
        let (token_pair, volume) = warp::test::request()
            .path("/markets/0-65535/estimated-buy-amount/1")
            .filter(&estimated_buy_amount())
            .now_or_never()
            .unwrap()
            .unwrap();
        assert_eq!(token_pair.buy_token_id, 0);
        assert_eq!(token_pair.sell_token_id, 65535);
        assert_eq!(volume, 1);
    }

    #[test]
    fn estimated_buy_amount_too_few_tokens() {
        for path in &[
            "/markets//estimated-buy-amount/1",
            "/markets/0/estimated-buy-amount/1",
        ] {
            assert!(warp::test::request()
                .path(path)
                .filter(&estimated_buy_amount())
                .now_or_never()
                .unwrap()
                .is_err());
        }
    }

    #[test]
    fn estimated_buy_amount_too_many_tokens() {
        for path in &[
            "/markets/0-1-2/estimated-buy-amount/1",
            "/markets/0-1-asdf/estimated-buy-amount/1",
            "/markets/0-1-2-3/estimated-buy-amount/1",
            "/markets/0-1-/estimated-buy-amount/1",
        ] {
            assert!(warp::test::request()
                .path(path)
                .filter(&estimated_buy_amount())
                .now_or_never()
                .unwrap()
                .is_err());
        }
    }

    #[test]
    fn estimated_buy_amount_no_volume() {
        for path in &[
            "/markets/0-1/estimated-buy-amount/",
            "/markets/0-1/estimated-buy-amount/asdf",
        ] {
            assert!(warp::test::request()
                .path(path)
                .filter(&estimated_buy_amount())
                .now_or_never()
                .unwrap()
                .is_err());
        }
    }

    #[test]
    fn estimated_buy_amount_no_float_volume() {
        for path in &[
            "/markets/0-1/estimated-buy-amount/0.0",
            "/markets/0-1/estimated-buy-amount/1.0",
            "/markets/0-1/estimated-buy-amount/0.5",
        ] {
            assert!(warp::test::request()
                .path(path)
                .filter(&estimated_buy_amount())
                .now_or_never()
                .unwrap()
                .is_err());
        }
    }
}
