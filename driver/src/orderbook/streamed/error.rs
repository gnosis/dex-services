use thiserror::Error;

#[derive(Clone, Copy, Debug, Error)]
pub enum Error {
    #[error("unknown token")]
    UnknownToken,
    #[error("unknown order")]
    UnknownOrder,
    #[error("order already exists")]
    OrderAlreadyExists,
    #[error("math underflow")]
    MathUnderflow,
}
