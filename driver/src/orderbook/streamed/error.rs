use super::*;
use thiserror::Error;

#[derive(Clone, Copy, Debug, Error)]
pub enum Error {
    #[error("unknown token")]
    UnknownToken,
    #[error("unknown order")]
    UnknownOrder,
    #[error("order already exists")]
    OrderAlreadyExists,
    #[error("math under or overflow")]
    MathOverflow,
    #[error("withdraw batch does not match withdraw request {0}")]
    WithdrawEarlierThanRequested(BatchId),
    #[error("withdraw amount is larger than withdraw request {0}")]
    WithdrawMoreThanRequested(U256),
    #[error("withdraw amount is larger than balance {0}")]
    WithdrawMoreThanBalance(U256),
}
