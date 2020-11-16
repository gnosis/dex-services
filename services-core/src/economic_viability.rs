//! Module implementing minimum average fee computation based on reference token
//! price estimates.

use crate::{gas_price::GasPriceEstimating, models::solution::EconomicViabilityInfo};
use anyhow::{anyhow, Context as _, Result};
use std::{num::NonZeroU128, sync::Arc};

/// The approximate amount of gas used in a solution per trade. In practice the value depends on how
/// much gas is used in the reversion of the previous solution.
const GAS_PER_TRADE: f64 = 120_000.0;

arg_enum! {
    #[derive(Debug)]
    pub enum EconomicViabilityStrategy {
        Static,
        Dynamic,
        Combined,
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
        static_min_avg_fee_per_order: Option<u128>,
        static_max_gas_price: Option<u128>,
        native_token_price: Arc<dyn NativeTokenPricing + Send + Sync>,
        gas_station: Arc<dyn GasPriceEstimating>,
    ) -> Result<Arc<dyn EconomicViabilityComputing>> {
        let make_dynamic = || {
            DynamicEconomicViabilityComputer::new(
                native_token_price,
                gas_station,
                subsidy_factor,
                min_avg_fee_factor,
            )
        };
        let make_fixed = || -> Result<_> {
            let min_avg_fee =
                static_min_avg_fee_per_order.ok_or_else(|| anyhow!("no min_avg_fee passed."))?;
            let max_gas_price =
                static_max_gas_price.ok_or_else(|| anyhow!("no max_gas_price passed."))?;
            Ok(FixedEconomicViabilityComputer::new(
                min_avg_fee,
                max_gas_price as f64,
            ))
        };
        Ok(match self {
            Self::Dynamic => Arc::new(make_dynamic()),
            Self::Static => Arc::new(make_fixed()?),
            Self::Combined => {
                let fixed = make_fixed()?;
                let dynamic = Box::new(make_dynamic());
                Arc::new(CombinedEconomicViabilityComputer { fixed, dynamic })
            }
        })
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
    async fn max_gas_price(&self, economic_viability_info: EconomicViabilityInfo) -> Result<f64>;
}

/// Economic viability constraints based on the current gas and native token price.
pub struct DynamicEconomicViabilityComputer {
    price_oracle: Arc<dyn NativeTokenPricing + Send + Sync>,
    gas_station: Arc<dyn GasPriceEstimating>,
    subsidy_factor: f64,
    /// We multiply the min average fee by this amount to ensure that if a solution has this minimum
    /// amount it will still be end up economically viable even when the gas or native token price moves
    /// slightly between solution computation and submission.
    min_avg_fee_factor: f64,
}

impl DynamicEconomicViabilityComputer {
    pub fn new(
        price_oracle: Arc<dyn NativeTokenPricing + Send + Sync>,
        gas_station: Arc<dyn GasPriceEstimating>,
        subsidy_factor: f64,
        min_avg_fee_factor: f64,
    ) -> Self {
        DynamicEconomicViabilityComputer {
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
            .estimate()
            .await
            .context("failed to get gas price")?;
        Ok(gas_price)
    }
}

#[async_trait::async_trait]
impl EconomicViabilityComputing for DynamicEconomicViabilityComputer {
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

    async fn max_gas_price(&self, economic_viability_info: EconomicViabilityInfo) -> Result<f64> {
        let earned_fee = economic_viability_info.earned_fee.to_f64_lossy();
        let num_trades = economic_viability_info.num_executed_orders;
        let native_token_price = self.native_token_price_in_owl().await?;
        let cap = gas_price_cap(native_token_price, earned_fee, num_trades);
        let subsidized = cap * self.subsidy_factor;
        log::debug!(
                "computed max gas price to be {} subsidized to {} based on earned fee {} num trades {} native token price {}",
                cap, subsidized, earned_fee, num_trades, native_token_price
            );
        Ok(subsidized)
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
    min_average_fee: u128,
    max_gas_price: f64,
}

impl FixedEconomicViabilityComputer {
    pub fn new(min_average_fee: u128, max_gas_price: f64) -> Self {
        Self {
            min_average_fee,
            max_gas_price,
        }
    }
}

#[async_trait::async_trait]
impl EconomicViabilityComputing for FixedEconomicViabilityComputer {
    async fn min_average_fee(&self) -> Result<u128> {
        Ok(self.min_average_fee)
    }

    async fn max_gas_price(&self, _: EconomicViabilityInfo) -> Result<f64> {
        Ok(self.max_gas_price)
    }
}

/// Use whichever computer yields a more trader friendly min average fee.
pub struct CombinedEconomicViabilityComputer {
    fixed: FixedEconomicViabilityComputer,
    // This is always DynamicViabilityComputing but for easier mock testing we use box.
    dynamic: Box<dyn EconomicViabilityComputing>,
}

#[async_trait::async_trait]
impl EconomicViabilityComputing for CombinedEconomicViabilityComputer {
    async fn min_average_fee(&self) -> Result<u128> {
        Ok(match self.dynamic.min_average_fee().await {
            Ok(fee) => fee.min(self.fixed.min_average_fee),
            Err(err) => {
                log::warn!("dynamic economic viability failed: {}", err);
                self.fixed.min_average_fee
            }
        })
    }

    async fn max_gas_price(&self, economic_viability_info: EconomicViabilityInfo) -> Result<f64> {
        let avg_fee = economic_viability_info.earned_fee.to_f64_lossy()
            / economic_viability_info.num_executed_orders as f64;
        // If the real average fee is worse than the fallback min average fee then we must have used
        // the dynamic computer for min_average_fee so we use it again for the max gas price.
        if avg_fee < self.fixed.min_average_fee as f64 {
            self.dynamic.max_gas_price(economic_viability_info).await
        // If the real fee is >= the fallback min average fee we could have used either computer so
        // there is nothing wrong with using the fallback max gas price even though it might be
        // bigger than the dynamic's computer max gas price.
        } else {
            Ok(self.fixed.max_gas_price)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{gas_price::MockGasPriceEstimating, util::FutureWaitExt as _};
    use assert_approx_eq::assert_approx_eq;
    use ethcontract::U256;
    use futures::FutureExt as _;

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
        gas_station.expect_estimate().returning(|| Ok(40e9));
        let subsidy = 10.0f64;
        let min_avg_fee_factor = 1.1f64;
        let economic_viability = DynamicEconomicViabilityComputer::new(
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
        assert_approx_eq!(
            economic_viability.max_gas_price(info).wait().unwrap(),
            5787037037037.037
        );
    }

    #[test]
    fn combined_strategy_picks_min_min_average_fee() {
        for (fixed_fee, dynamic_fee, expected_fee) in &[(5, 10, 5), (5, 1, 1)] {
            let fixed = FixedEconomicViabilityComputer::new(*fixed_fee, 0.into());
            let mut dynamic = MockEconomicViabilityComputing::new();
            dynamic
                .expect_min_average_fee()
                .times(1)
                .returning(move || Ok(*dynamic_fee));
            let combined = CombinedEconomicViabilityComputer {
                fixed,
                dynamic: Box::new(dynamic),
            };
            let result = combined.min_average_fee().now_or_never().unwrap().unwrap();
            assert_eq!(result, *expected_fee);
        }
    }

    #[test]
    fn combined_strategy_picks_correct_max_gas_price() {
        let fixed_gas = 1.0;
        let fixed_fee = 15u128;
        let dynamic_gas = 2.0;
        let fixed = FixedEconomicViabilityComputer::new(fixed_fee, fixed_gas);
        let mut dynamic = MockEconomicViabilityComputing::new();
        dynamic
            .expect_max_gas_price()
            .returning(move |_| Ok(dynamic_gas));
        let combined = CombinedEconomicViabilityComputer {
            fixed,
            dynamic: Box::new(dynamic),
        };
        // In the first run the average fee of the solution is >= the fixed_fee so the fixed gas
        // price should be chosen and in the second run < so the dynamic should be chosen.
        for (earned_fee, expected_gas) in &[(15, 1.0), (14, 2.0)] {
            let info = EconomicViabilityInfo {
                num_executed_orders: 1,
                earned_fee: U256::from(*earned_fee),
            };
            let result = combined
                .max_gas_price(info)
                .now_or_never()
                .unwrap()
                .unwrap();
            assert_approx_eq!(result, *expected_gas);
        }
    }
}
