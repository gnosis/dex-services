//! Module implementing minimum average fee computation based on reference token
//! price estimates.

use crate::{
    gas_station::GasPriceEstimating, models::solution::EconomicViabilityInfo,
    price_estimation::PriceEstimating,
};
use anyhow::{anyhow, Result};
use ethcontract::U256;
use futures::future::{BoxFuture, FutureExt as _};
use std::sync::Arc;

/// The approximate amount of gas used in a solution per trade. In practice the value depends on how
/// much gas is used in the reversion of the previous solution.
const GAS_PER_TRADE: f64 = 120_000.0;

#[cfg_attr(test, mockall::automock)]
pub trait EconomicViabilityComputing: Send + Sync + 'static {
    /// Used by the solver so that it only considers solution that are economically viable.
    fn min_average_fee<'a>(&'a self) -> BoxFuture<'a, Result<u128>>;
    /// The maximum gas price at which submitting the solution is still economically viable.
    fn max_gas_price<'a>(
        &'a self,
        economic_viability_info: EconomicViabilityInfo,
    ) -> BoxFuture<'a, Result<U256>>;
}

/// Economic viability constraints based on the current gas and eth price.
pub struct EconomicViabilityComputer {
    price_oracle: Arc<dyn PriceEstimating + Send + Sync>,
    gas_station: Arc<dyn GasPriceEstimating + Send + Sync>,
    subsidy_factor: f64,
}

impl EconomicViabilityComputer {
    pub fn new(
        price_oracle: Arc<dyn PriceEstimating + Send + Sync>,
        gas_station: Arc<dyn GasPriceEstimating + Send + Sync>,
        subsidy_factor: f64,
    ) -> Self {
        EconomicViabilityComputer {
            price_oracle,
            gas_station,
            subsidy_factor,
        }
    }

    async fn eth_price_in_owl(&self) -> Result<f64> {
        self.price_oracle
            .get_eth_price()
            .await
            .map(|price| price.get() as f64)
            .ok_or_else(|| anyhow!("failed to find ETH price estimate"))
    }

    async fn gas_price(&self) -> Result<f64> {
        let fast = self.gas_station.estimate_gas_price().await?.fast;
        Ok(pricegraph::num::u256_to_f64(fast))
    }
}

impl EconomicViabilityComputing for EconomicViabilityComputer {
    fn min_average_fee(&self) -> BoxFuture<'_, Result<u128>> {
        async move {
            let eth_price = self.eth_price_in_owl().await?;
            let gas_price = self.gas_price().await?;

            let fee = min_average_fee(eth_price, gas_price);
            let subsidized = fee / self.subsidy_factor;
            log::debug!(
                "computed min average fee to be {}, subsidized to {}",
                fee,
                subsidized,
            );

            Ok(subsidized as _)
        }
        .boxed()
    }

    fn max_gas_price<'a>(
        &'a self,
        economic_viability_info: EconomicViabilityInfo,
    ) -> BoxFuture<'a, Result<U256>> {
        async move {
            let earned_fee = pricegraph::num::u256_to_f64(economic_viability_info.earned_fee);
            let num_trades = economic_viability_info.num_executed_orders;
            let eth_price = self.eth_price_in_owl().await?;
            let cap = gas_price_cap(eth_price, earned_fee, num_trades);
            let subsidized = cap * self.subsidy_factor;
            Ok(U256::from(subsidized as u128))
        }
        .boxed()
    }
}

/// Computes the min average fee per order based on the current ETH price in
/// reference token and a gas price estimate. Returns the minimum average fee
/// in reference token that must be accumulated per order in order for a
/// solution to be economically viable.
fn min_average_fee(eth_price: f64, gas_price: f64) -> f64 {
    let owl_per_eth = eth_price / 1e18;
    let gas_price_in_owl = owl_per_eth * gas_price;
    GAS_PER_TRADE * gas_price_in_owl
}

/// The gas price cap is selected so that submitting solution is still roughly profitable.
fn gas_price_cap(eth_price: f64, earned_fee: f64, num_trades: usize) -> f64 {
    let owl_per_eth = eth_price / 1e18;
    let gas_use = GAS_PER_TRADE * (num_trades as f64);
    earned_fee / (owl_per_eth * gas_use)
}

/// Fixed values.
pub struct FixedEconomicViabilityComputer {
    min_average_fee: Option<u128>,
    max_gas_price: Option<U256>,
}

impl FixedEconomicViabilityComputer {
    pub fn new(min_average_fee: Option<u128>, max_gas_price: Option<U256>) -> Self {
        Self {
            min_average_fee,
            max_gas_price,
        }
    }
}

impl EconomicViabilityComputing for FixedEconomicViabilityComputer {
    fn min_average_fee(&self) -> BoxFuture<'_, Result<u128>> {
        immediate!(self
            .min_average_fee
            .ok_or_else(|| anyhow!("no min average fee set")))
    }

    fn max_gas_price<'a>(&'a self, _: EconomicViabilityInfo) -> BoxFuture<'a, Result<U256>> {
        immediate!(self
            .max_gas_price
            .ok_or_else(|| anyhow!("no max gas price set")))
    }
}

/// Takes the first successful inner computer.
pub struct PriorityEconomicViabilityComputer(Vec<Box<dyn EconomicViabilityComputing>>);

impl PriorityEconomicViabilityComputer {
    /// Creates a new priority minimum average fee computing from the specified
    /// implementations.
    pub fn new(inner: Vec<Box<dyn EconomicViabilityComputing>>) -> Self {
        PriorityEconomicViabilityComputer(inner)
    }

    async fn until_success<T>(
        &self,
        mut operation: impl FnMut(&dyn EconomicViabilityComputing) -> BoxFuture<Result<T>>,
    ) -> Result<T> {
        for inner in self.0.iter() {
            match operation(inner.as_ref()).await {
                Ok(value) => return Ok(value),
                Err(err) => log::warn!("failed operation: {:?}", err),
            }
        }
        Err(anyhow!(
            "failed operation with all internal implementations"
        ))
    }
}

impl EconomicViabilityComputing for PriorityEconomicViabilityComputer {
    fn min_average_fee(&self) -> BoxFuture<'_, Result<u128>> {
        self.until_success(EconomicViabilityComputing::min_average_fee)
            .boxed()
    }

    fn max_gas_price<'a>(
        &'a self,
        economic_viability_info: EconomicViabilityInfo,
    ) -> BoxFuture<'a, Result<U256>> {
        self.until_success(move |computer| computer.max_gas_price(economic_viability_info))
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
        let gas_price = 40e9;
        let eth_price = 240e18;
        assert_approx_eq!(min_average_fee(eth_price, gas_price), 1152e15);
    }

    #[test]
    fn computes_gas_price_cap() {
        // 50 owl fee, ~600 gwei gas price cap
        assert_approx_eq!(gas_price_cap(240e18, 50e18, 3), 578703703703.7037);
    }

    #[test]
    fn uses_gas_and_eth_price_estimates_with_subsidy() {
        let mut price_oracle = MockPriceEstimating::new();
        price_oracle
            .expect_get_eth_price()
            .returning(|| immediate!(Some(nonzero!(240e18 as u128))));

        let mut gas_station = MockGasPriceEstimating::new();
        gas_station.expect_estimate_gas_price().returning(|| {
            immediate!(Ok(GasPrice {
                fast: (40e9 as u128).into(),
                ..Default::default()
            }))
        });
        let economic_viability =
            EconomicViabilityComputer::new(Arc::new(price_oracle), Arc::new(gas_station), 10.0);

        assert_eq!(
            economic_viability.min_average_fee().wait().unwrap(),
            1152e14 as u128, // 0.1152 OWL
        );

        let info = EconomicViabilityInfo {
            num_executed_orders: 3,
            earned_fee: U256::from(50e18 as u128),
        };
        assert_eq!(
            economic_viability.max_gas_price(info).wait().unwrap(),
            U256::from(5787037037037u128)
        );
    }

    #[test]
    fn priority_impl_takes_first_success() {
        let priority_min_avg_fee = PriorityEconomicViabilityComputer::new(
            vec![Err(anyhow!("some error")), Ok(42), Ok(1337)]
                .into_iter()
                .map(|result| -> Box<dyn EconomicViabilityComputing> {
                    let mut mock = MockEconomicViabilityComputing::new();
                    mock.expect_min_average_fee()
                        .return_once(move || immediate!(result));
                    Box::new(mock)
                })
                .collect(),
        );

        assert_eq!(priority_min_avg_fee.min_average_fee().wait().unwrap(), 42);
    }
}
