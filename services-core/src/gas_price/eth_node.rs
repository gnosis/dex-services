//! Ethereum node `GasPriceEstimating` implementation.

use super::GasPriceEstimating;
use crate::contracts::Web3;
use anyhow::Result;
use ethcontract::U256;
use futures::{
    compat::Future01CompatExt,
    future::{BoxFuture, FutureExt as _, TryFutureExt as _},
};

impl GasPriceEstimating for Web3 {
    fn estimate_gas_price(&self) -> BoxFuture<Result<U256>> {
        self.eth().gas_price().compat().map_err(From::from).boxed()
    }
}
