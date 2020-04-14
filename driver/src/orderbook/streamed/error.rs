use super::*;
use thiserror::Error;

#[derive(Clone, Copy, Debug, Error)]
pub enum Error {
    #[error("unknown token {0}")]
    UnknownToken(TokenId),
    #[error("unknown order {0}")]
    UnknownOrder(OrderId),
    #[error("order already exists")]
    OrderAlreadyExists,
    #[error("math over or underflow")]
    MathOverflow,
    #[error("solution submitted but there is no fee token")]
    SolutionWithoutFeeToken,
    #[error("order deletion for order that is still valid")]
    DeletingValidOrder,
    #[error("trade reversion does not match previous trade")]
    RevertingNonExistentTrade,
    #[error("withdraw batch does not match withdraw request {0}")]
    WithdrawEarlierThanRequested(BatchId),
    #[error("withdraw amount is larger than withdraw request {0}")]
    WithdrawMoreThanRequested(U256),
    #[error("withdraw amount is larger than balance {0}")]
    WithdrawMoreThanBalance(U256),
    #[error("trade for batch that no longer accepts solutions")]
    TradeForPastBatch,
    #[error("solution for batch that no longer accepts solutions")]
    SolutionForPastBatch,
    #[error("trade increases used amount of order by more than the order's limit {0}")]
    TradeByMoreThanOrderLimit(u128),
}
