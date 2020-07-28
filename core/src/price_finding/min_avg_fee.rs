//! Module implementing minimum average fee computation based on reference token
//! price estimates.

use crate::{gas_station::GasPriceEstimating, price_estimation::PriceEstimating};
use anyhow::{anyhow, Result};
use futures::future::{BoxFuture, FutureExt as _};
use std::sync::Arc;

/// Trait that abstracts the minimum average fee computation.
#[cfg_attr(test, mockall::automock)]
pub trait MinAverageFeeComputing: Send + Sync + 'static {
    /// Retrieves the current minimum average fee.
    fn current<'a>(&'a self) -> BoxFuture<'a, Result<u128>>;
}

/// Implementation for computing the minumum average fee based on an approximate
/// gas amount per touched order, current gas price estimates and and ETH price
/// estimates.
pub struct ApproximateMinAverageFee {
    price_oracle: Arc<dyn PriceEstimating + Send + Sync>,
    gas_station: Arc<dyn GasPriceEstimating + Send + Sync>,
    subsidy_factor: f64,
}

impl ApproximateMinAverageFee {
    pub fn new(
        price_oracle: Arc<dyn PriceEstimating + Send + Sync>,
        gas_station: Arc<dyn GasPriceEstimating + Send + Sync>,
        subsidy_factor: f64,
    ) -> Self {
        ApproximateMinAverageFee {
            price_oracle,
            gas_station,
            subsidy_factor,
        }
    }
}

impl MinAverageFeeComputing for ApproximateMinAverageFee {
    fn current(&self) -> BoxFuture<'_, Result<u128>> {
        async move {
            let eth_price_in_owl = self
                .price_oracle
                .get_eth_price()
                .await
                .ok_or_else(|| anyhow!("failed to find ETH price estimate"))?;

            let fast_gas_price = self.gas_station.estimate_gas_price().await?.fast;

            let fee = compute_min_average_fee(
                eth_price_in_owl as _,
                pricegraph::num::u256_to_f64(fast_gas_price),
            );
            let subsidized_fee = fee / self.subsidy_factor;
            log::debug!(
                "computed min average fee to be {}, subsidized to {}",
                fee,
                subsidized_fee,
            );

            Ok(subsidized_fee as _)
        }
        .boxed()
    }
}

/// Computes the min average fee per order based on the current ETH price in
/// reference token and a gas price estimate. Returns the minimum average fee
/// in reference token that must be accumulated per order in order for a
/// solution to be economically viable.
fn compute_min_average_fee(eth_price: f64, gas_price: f64) -> f64 {
    const GAS_PER_ORDER: f64 = 120_000.0;

    let owl_per_eth = eth_price / 10f64.powi(18);
    let gas_price_in_owl = owl_per_eth * gas_price;

    GAS_PER_ORDER * gas_price_in_owl
}

/// Fixed minimum average fee.
pub struct FixedMinAverageFee(pub u128);

impl MinAverageFeeComputing for FixedMinAverageFee {
    fn current(&self) -> BoxFuture<'_, Result<u128>> {
        immediate!(Ok(self.0))
    }
}

/// Priority minimum average fee computing that takes the first successfully
/// computed minimum average fee.
pub struct PriorityMinAverageFee(Vec<Box<dyn MinAverageFeeComputing>>);

impl PriorityMinAverageFee {
    /// Creates a new priority minimum average fee computing from the specified
    /// implementations.
    pub fn new(inner: Vec<Box<dyn MinAverageFeeComputing>>) -> Self {
        PriorityMinAverageFee(inner)
    }
}

impl MinAverageFeeComputing for PriorityMinAverageFee {
    fn current(&self) -> BoxFuture<'_, Result<u128>> {
        async move {
            for inner in self.0.iter() {
                match inner.current().await {
                    Ok(value) => return Ok(value),
                    Err(err) => log::warn!("failed to compute minimum average fee: {:?}", err),
                }
            }

            Err(anyhow!(
                "failure computing minimum average fee with all internal implementations"
            ))
        }
        .boxed()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        gas_station::{GasPrice, MockGasPriceEstimating},
        price_estimation::MockPriceEstimating,
        util::FutureWaitExt as _,
    };
    use assert_approx_eq::assert_approx_eq;

    #[test]
    fn computes_min_average_fee() {
        let gas_price = 40.0 * 10.0f64.powi(9);
        let eth_price = 240.0 * 10.0f64.powi(18);

        assert_approx_eq!(
            compute_min_average_fee(eth_price, gas_price),
            1_152_000_000_000_000_000.0
        );
    }

    #[test]
    fn uses_gas_and_eth_price_estimates_with_subsidy() {
        let mut price_oracle = MockPriceEstimating::new();
        price_oracle
            .expect_get_eth_price()
            .returning(|| immediate!(Some(240 * 10u128.pow(18))));

        let mut gas_station = MockGasPriceEstimating::new();
        gas_station.expect_estimate_gas_price().returning(|| {
            immediate!(Ok(GasPrice {
                fast: (40 * 10u128.pow(9)).into(),
                ..Default::default()
            }))
        });

        let min_avg_fee =
            ApproximateMinAverageFee::new(Arc::new(price_oracle), Arc::new(gas_station), 10.0);
        assert_eq!(
            min_avg_fee.current().wait().unwrap(),
            115_200_000_000_000_000, // 0.1152 OWL
        );
    }

    #[test]
    fn priority_impl_takes_first_success() {
        let priority_min_avg_fee = PriorityMinAverageFee::new(
            vec![Err(anyhow!("some error")), Ok(42), Ok(1337)]
                .into_iter()
                .map(|result| -> Box<dyn MinAverageFeeComputing> {
                    let mut mock = MockMinAverageFeeComputing::new();
                    mock.expect_current()
                        .return_once(move || immediate!(result));
                    Box::new(mock)
                })
                .collect(),
        );

        assert_eq!(priority_min_avg_fee.current().wait().unwrap(), 42);
    }
}
