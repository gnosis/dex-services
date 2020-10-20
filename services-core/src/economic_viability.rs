//! Module implementing minimum average fee computation based on reference token
//! price estimates.

use crate::{gas_price::GasPriceEstimating, models::solution::EconomicViabilityInfo};
use anyhow::{anyhow, Context as _, Result};
use ethcontract::U256;
use futures::future;
use std::{num::NonZeroU128, sync::Arc};

/// The approximate amount of gas used in a solution per trade. In practice the value depends on how
/// much gas is used in the reversion of the previous solution.
const GAS_PER_TRADE: f64 = 120_000.0;

arg_enum! {
    #[derive(Debug)]
    pub enum EconomicViabilityStrategy {
        Static,
        Dynamic,
        DynamicBoundedByStatic,
    }
}

impl EconomicViabilityStrategy {
    /// Create a economic viability instance as commonly used from command line arguments.
    /// If the strategy is dynamic use a priority source of the subsidy factor and fallback,
    /// if static use only the fallback.
    pub fn from_arguments(
        &self,
        subsidy_factor: f64,
        min_avg_fee_factor: f64,
        fallback_min_avg_fee_per_order: u128,
        fallback_max_gas_price: u128,
        native_token_price: Arc<dyn NativeTokenPricing + Send + Sync>,
        gas_station: Arc<dyn GasPriceEstimating + Send + Sync>,
    ) -> Arc<dyn EconomicViabilityComputing> {
        let fixed = FixedEconomicViabilityComputer::new(
            Some(fallback_min_avg_fee_per_order),
            Some(fallback_max_gas_price.into()),
        );
        let dynamic = EconomicViabilityComputer::new(
            native_token_price,
            gas_station,
            subsidy_factor,
            min_avg_fee_factor,
        );
        match self {
            EconomicViabilityStrategy::Static => Arc::new(fixed),
            EconomicViabilityStrategy::Dynamic => {
                Arc::new(PriorityEconomicViabilityComputer::new(vec![
                    Box::new(dynamic),
                    Box::new(fixed),
                ]))
            }
            EconomicViabilityStrategy::DynamicBoundedByStatic => Arc::new(
                BoundedEconomicViabilityComputer::new(Box::new(dynamic), Box::new(fixed)),
            ),
        }
    }
}

#[cfg_attr(test, mockall::automock)]
#[async_trait::async_trait]
pub trait NativeTokenPricing {
    /// Retrieves a price estimate for ETH in OWL atoms.
    /// The amount of OWL in atoms to purchase 1.0 ETH (or 1e18 wei).
    async fn get_native_token_price(&self) -> Option<NonZeroU128>;
}

#[cfg_attr(test, mockall::automock)]
#[async_trait::async_trait]
pub trait EconomicViabilityComputing: Send + Sync + 'static {
    /// Used by the solver so that it only considers solution that are economically viable.
    /// This is the minimum average amount of earned fees per order. The total amount of paid fees
    /// is twice this because half of the fee is burnt.
    async fn min_average_fee(&self) -> Result<u128>;
    /// The maximum gas price at which submitting the solution is still economically viable.
    async fn max_gas_price(&self, economic_viability_info: EconomicViabilityInfo) -> Result<U256>;
}

/// Economic viability constraints based on the current gas and native token price.
pub struct EconomicViabilityComputer {
    price_oracle: Arc<dyn NativeTokenPricing + Send + Sync>,
    gas_station: Arc<dyn GasPriceEstimating + Send + Sync>,
    subsidy_factor: f64,
    /// We multiply the min average fee by this amount to ensure that if a solution has this minimum
    /// amount it will still be end up economically viable even when the gas or native token price moves
    /// slightly between solution computation and submission.
    min_avg_fee_factor: f64,
}

impl EconomicViabilityComputer {
    pub fn new(
        price_oracle: Arc<dyn NativeTokenPricing + Send + Sync>,
        gas_station: Arc<dyn GasPriceEstimating + Send + Sync>,
        subsidy_factor: f64,
        min_avg_fee_factor: f64,
    ) -> Self {
        EconomicViabilityComputer {
            price_oracle,
            gas_station,
            subsidy_factor,
            min_avg_fee_factor,
        }
    }

    async fn native_token_price_in_owl(&self) -> Result<f64> {
        self.price_oracle
            .get_native_token_price()
            .await
            .map(|price| price.get() as f64)
            .ok_or_else(|| anyhow!("failed to find native token price estimate"))
    }

    async fn gas_price(&self) -> Result<f64> {
        let gas_price = self
            .gas_station
            .estimate_gas_price()
            .await
            .context("failed to get gas price")?;
        Ok(pricegraph::num::u256_to_f64(gas_price))
    }
}

#[async_trait::async_trait]
impl EconomicViabilityComputing for EconomicViabilityComputer {
    async fn min_average_fee(&self) -> Result<u128> {
        let native_token_price = self.native_token_price_in_owl().await?;
        let gas_price = self.gas_price().await?;

        let fee = min_average_fee(native_token_price, gas_price) * self.min_avg_fee_factor;
        let subsidized = fee / self.subsidy_factor;
        log::debug!(
                "computed min average fee to be {}, subsidized to {} based on native token price {} gas price {}",
                fee, subsidized, native_token_price, gas_price
            );

        Ok(subsidized as _)
    }

    async fn max_gas_price(&self, economic_viability_info: EconomicViabilityInfo) -> Result<U256> {
        let earned_fee = pricegraph::num::u256_to_f64(economic_viability_info.earned_fee);
        let num_trades = economic_viability_info.num_executed_orders;
        let native_token_price = self.native_token_price_in_owl().await?;
        let cap = gas_price_cap(native_token_price, earned_fee, num_trades);
        let subsidized = cap * self.subsidy_factor;
        log::debug!(
                "computed max gas price to be {} subsidized to {} based on earned fee {} num trades {} native token price {}",
                cap, subsidized, earned_fee, num_trades, native_token_price
            );
        Ok(U256::from(subsidized as u128))
    }
}

/// Computes the min average fee per order based on the current native token price in
/// reference token and a gas price estimate. Returns the minimum average fee
/// in reference token that must be accumulated per order in order for a
/// solution to be economically viable.
fn min_average_fee(native_token_price: f64, gas_price: f64) -> f64 {
    let owl_per_eth = native_token_price / 1e18;
    let gas_price_in_owl = owl_per_eth * gas_price;
    GAS_PER_TRADE * gas_price_in_owl
}

/// The gas price cap is selected so that submitting solution is still roughly profitable.
fn gas_price_cap(native_token_price: f64, earned_fee: f64, num_trades: usize) -> f64 {
    let owl_per_eth = native_token_price / 1e18;
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

#[async_trait::async_trait]
impl EconomicViabilityComputing for FixedEconomicViabilityComputer {
    async fn min_average_fee(&self) -> Result<u128> {
        self.min_average_fee
            .ok_or_else(|| anyhow!("no min average fee set"))
    }

    async fn max_gas_price(&self, _: EconomicViabilityInfo) -> Result<U256> {
        self.max_gas_price
            .ok_or_else(|| anyhow!("no max gas price set"))
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

    async fn until_success<'a, T, Future>(
        &'a self,
        mut operation: impl FnMut(&'a dyn EconomicViabilityComputing) -> Future + 'a,
    ) -> Result<T>
    where
        Future: std::future::Future<Output = Result<T>>,
    {
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

#[async_trait::async_trait]
impl EconomicViabilityComputing for PriorityEconomicViabilityComputer {
    async fn min_average_fee(&self) -> Result<u128> {
        self.until_success(EconomicViabilityComputing::min_average_fee)
            .await
    }

    async fn max_gas_price(&self, economic_viability_info: EconomicViabilityInfo) -> Result<U256> {
        self.until_success(move |computer| computer.max_gas_price(economic_viability_info))
            .await
    }
}

/// Use the better result of two computers. If first fails second is used.
/// For min-avg-fee lower is better, for max-gas-price higher is better.
pub struct BoundedEconomicViabilityComputer {
    first: Box<dyn EconomicViabilityComputing>,
    second: Box<dyn EconomicViabilityComputing>,
}

impl BoundedEconomicViabilityComputer {
    pub fn new(
        first: Box<dyn EconomicViabilityComputing>,
        second: Box<dyn EconomicViabilityComputing>,
    ) -> Self {
        Self { first, second }
    }

    async fn bounded<'a, T, Future>(
        &'a self,
        operation: impl Fn(&'a dyn EconomicViabilityComputing) -> Future + 'a,
        comparison: impl Fn(T, T) -> T,
    ) -> Result<T>
    where
        Future: std::future::Future<Output = Result<T>>,
    {
        let (first, second) = future::join(
            operation(self.first.as_ref()),
            operation(self.second.as_ref()),
        )
        .await;
        let second = second?;
        Ok(match first {
            Ok(first) => comparison(first, second),
            Err(err) => {
                log::warn!("failed first operation: {:?}", err);
                second
            }
        })
    }
}

#[async_trait::async_trait]
impl EconomicViabilityComputing for BoundedEconomicViabilityComputer {
    async fn min_average_fee(&self) -> Result<u128> {
        self.bounded(EconomicViabilityComputing::min_average_fee, std::cmp::min)
            .await
    }

    async fn max_gas_price(&self, economic_viability_info: EconomicViabilityInfo) -> Result<U256> {
        self.bounded(
            move |computer| computer.max_gas_price(economic_viability_info),
            std::cmp::max,
        )
        .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{gas_price::MockGasPriceEstimating, util::FutureWaitExt as _};
    use assert_approx_eq::assert_approx_eq;
    use futures::future::FutureExt as _;

    #[test]
    fn computes_min_average_fee() {
        let gas_price = 40e9;
        let native_token_price = 240e18;
        assert_approx_eq!(min_average_fee(native_token_price, gas_price), 1152e15);
    }

    #[test]
    fn computes_gas_price_cap() {
        // 50 owl fee, ~600 gwei gas price cap
        assert_approx_eq!(gas_price_cap(240e18, 50e18, 3), 578703703703.7037);
    }

    #[test]
    fn uses_gas_and_native_token_price_estimates_with_subsidy() {
        let mut price_oracle = MockNativeTokenPricing::new();
        price_oracle
            .expect_get_native_token_price()
            .returning(|| Some(nonzero!(240e18 as u128)));

        let mut gas_station = MockGasPriceEstimating::new();
        gas_station
            .expect_estimate_gas_price()
            .returning(|| Ok((40e9 as u128).into()));
        let subsidy = 10.0f64;
        let min_avg_fee_factor = 1.1f64;
        let economic_viability = EconomicViabilityComputer::new(
            Arc::new(price_oracle),
            Arc::new(gas_station),
            subsidy,
            min_avg_fee_factor,
        );

        assert_eq!(
            economic_viability.min_average_fee().wait().unwrap(),
            ((1152e15 * min_avg_fee_factor) / subsidy) as u128, // 0.1152 OWL
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
                    mock.expect_min_average_fee().return_once(move || result);
                    Box::new(mock)
                })
                .collect(),
        );

        assert_eq!(priority_min_avg_fee.min_average_fee().wait().unwrap(), 42);
    }

    #[test]
    fn bounded_uses_min_of_first_and_second_min_avg_fee() {
        let mut first = MockEconomicViabilityComputing::new();
        first.expect_min_average_fee().return_once(|| Ok(10));
        let mut second = MockEconomicViabilityComputing::new();
        second.expect_min_average_fee().return_once(|| Ok(5));
        let bounded = BoundedEconomicViabilityComputer::new(Box::new(first), Box::new(second));
        assert_eq!(
            bounded.min_average_fee().now_or_never().unwrap().unwrap(),
            5
        );
    }

    #[test]
    fn bounded_uses_max_of_first_and_second_max_gas_price() {
        let mut first = MockEconomicViabilityComputing::new();
        first.expect_max_gas_price().return_once(|_| Ok(10.into()));
        let mut second = MockEconomicViabilityComputing::new();
        second.expect_max_gas_price().return_once(|_| Ok(5.into()));
        let bounded = BoundedEconomicViabilityComputer::new(Box::new(first), Box::new(second));
        assert_eq!(
            bounded
                .max_gas_price(Default::default())
                .now_or_never()
                .unwrap()
                .unwrap(),
            10.into(),
        );
    }

    #[test]
    fn bounded_fails_if_second_fails() {
        let mut first = MockEconomicViabilityComputing::new();
        first.expect_min_average_fee().return_once(|| Ok(10));
        let mut second = MockEconomicViabilityComputing::new();
        second
            .expect_min_average_fee()
            .return_once(|| Err(anyhow!("")));
        let bounded = BoundedEconomicViabilityComputer::new(Box::new(first), Box::new(second));
        assert!(bounded.min_average_fee().now_or_never().unwrap().is_err());
    }

    #[test]
    fn bounded_uses_second_on_error() {
        let mut first = MockEconomicViabilityComputing::new();
        first
            .expect_min_average_fee()
            .return_once(|| Err(anyhow!("")));
        let mut second = MockEconomicViabilityComputing::new();
        second.expect_min_average_fee().return_once(|| Ok(5));
        let bounded = BoundedEconomicViabilityComputer::new(Box::new(first), Box::new(second));
        assert_eq!(
            bounded.min_average_fee().now_or_never().unwrap().unwrap(),
            5
        );
    }
}
