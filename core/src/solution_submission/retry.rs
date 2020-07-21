use crate::{
    contracts::stablex_contract::StableXContract, gas_station::GasPriceEstimating, models::Solution,
};
use anyhow::Result;
use ethcontract::{
    errors::{ExecutionError, MethodError},
    U256,
};
use pricegraph::num;

use super::MIN_GAS_PRICE_INCREASE_FACTOR;

const DEFAULT_GAS_PRICE: u64 = 15_000_000_000;

fn is_confirm_timeout(result: &Result<(), MethodError>) -> bool {
    matches!(
        result,
        &Err(MethodError {
            inner: ExecutionError::ConfirmTimeout,
            ..
        })
    )
}

struct InfallibleGasPriceEstimator<'a> {
    gas_price_estimating: &'a (dyn GasPriceEstimating + Sync),
    previous_gas_price: U256,
}

impl<'a> InfallibleGasPriceEstimator<'a> {
    fn new(
        gas_price_estimating: &'a (dyn GasPriceEstimating + Sync),
        default_gas_price: U256,
    ) -> Self {
        Self {
            gas_price_estimating,
            previous_gas_price: default_gas_price,
        }
    }

    /// Get a fresh price estimate or if that fails return the most recent previous result.
    async fn estimate(&mut self) -> U256 {
        match self.gas_price_estimating.estimate_gas_price().await {
            Ok(gas_estimate) => {
                // `retry` relies on the gas price always increasing.
                self.previous_gas_price = self.previous_gas_price.max(gas_estimate.fast);
            }
            Err(ref err) => {
                log::warn!(
                    "failed to get gas price from gnosis safe gas station: {}",
                    err
                );
            }
        };
        self.previous_gas_price
    }
}

fn gas_price(estimated_price: U256, price_increase_count: u32, cap: U256) -> U256 {
    let factor = 1.5f64.powf(price_increase_count as f64);
    let new_price = num::u256_to_f64(estimated_price) * factor;
    cap.min(U256::from(new_price as u128))
}

pub async fn retry_with_gas_price_increase(
    contract: &(dyn StableXContract + Sync),
    batch_index: u32,
    solution: Solution,
    claimed_objective_value: U256,
    gas_price_estimating: &(dyn GasPriceEstimating + Sync),
    gas_price_cap: U256,
    nonce: U256,
) -> Result<(), MethodError> {
    const BLOCK_TIMEOUT: usize = 2;

    let effective_gas_price_cap = U256::from(
        (gas_price_cap.as_u128() as f64 / MIN_GAS_PRICE_INCREASE_FACTOR).floor() as u128,
    );
    assert!(effective_gas_price_cap <= gas_price_cap);

    let mut gas_price_estimator =
        InfallibleGasPriceEstimator::new(gas_price_estimating, DEFAULT_GAS_PRICE.into());

    for gas_price_increase_count in 0u32.. {
        let estimated_price = gas_price_estimator.estimate().await;
        let gas_price = gas_price(estimated_price, gas_price_increase_count, gas_price_cap);
        assert!(gas_price <= gas_price_cap);
        log::info!(
            "solution submission try {} with gas price {}",
            gas_price_increase_count,
            gas_price
        );
        let is_last_iteration = gas_price >= effective_gas_price_cap;
        let block_timeout = if is_last_iteration {
            None
        } else {
            Some(BLOCK_TIMEOUT)
        };
        let result = contract
            .submit_solution(
                batch_index,
                solution.clone(),
                claimed_objective_value,
                gas_price,
                block_timeout,
                nonce,
            )
            .await;
        // Technically this being the last iteration implies there not being a confirm timeout so
        // we could drop the check for the last iteration but in practice it is more robust to check
        // this in case we unexpectedly do get a confirm timeout even though the block timeout is
        // not set.
        if !is_confirm_timeout(&result) || is_last_iteration {
            return result;
        }
    }
    unreachable!("increased gas price past expected limit");
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        contracts::stablex_contract::MockStableXContract,
        gas_station::{GasPrice, MockGasPriceEstimating},
    };
    use anyhow::anyhow;
    use futures::future::FutureExt as _;
    use mockall::predicate::*;

    #[test]
    fn infallible_gas_price_estimator_uses_default_and_previous_result() {
        let mut gas_station = MockGasPriceEstimating::new();
        gas_station
            .expect_estimate_gas_price()
            .times(1)
            .return_once(|| immediate!(Err(anyhow!(""))));
        gas_station
            .expect_estimate_gas_price()
            .times(1)
            .return_once(|| {
                immediate!(Ok(GasPrice {
                    fast: 5.into(),
                    ..Default::default()
                }))
            });
        gas_station
            .expect_estimate_gas_price()
            .times(1)
            .return_once(|| {
                immediate!(Ok(GasPrice {
                    fast: 6.into(),
                    ..Default::default()
                }))
            });
        gas_station
            .expect_estimate_gas_price()
            .times(1)
            .return_once(|| immediate!(Err(anyhow!(""))));

        let mut estimator = InfallibleGasPriceEstimator::new(&gas_station, 3.into());
        assert_eq!(estimator.estimate().now_or_never().unwrap(), U256::from(3));
        assert_eq!(estimator.estimate().now_or_never().unwrap(), U256::from(5));
        assert_eq!(estimator.estimate().now_or_never().unwrap(), U256::from(6));
        assert_eq!(estimator.estimate().now_or_never().unwrap(), U256::from(6));
    }

    #[test]
    fn infallible_gas_price_estimator_does_not_decrease() {
        let mut gas_station = MockGasPriceEstimating::new();
        gas_station
            .expect_estimate_gas_price()
            .times(1)
            .return_once(|| {
                immediate!(Ok(GasPrice {
                    fast: 10.into(),
                    ..Default::default()
                }))
            });
        gas_station
            .expect_estimate_gas_price()
            .times(1)
            .return_once(|| {
                immediate!(Ok(GasPrice {
                    fast: 9.into(),
                    ..Default::default()
                }))
            });

        let mut estimator = InfallibleGasPriceEstimator::new(&gas_station, 3.into());
        assert_eq!(estimator.estimate().now_or_never().unwrap(), U256::from(10));
        assert_eq!(estimator.estimate().now_or_never().unwrap(), U256::from(10));
    }

    #[test]
    fn gas_price_increases_as_expected_and_hits_limit() {
        let estimated = U256::from(5);
        let cap = U256::from(50);
        assert_eq!(gas_price(estimated, 0, cap), U256::from(5));
        assert_eq!(gas_price(estimated, 1, cap), U256::from(7));
        assert_eq!(gas_price(estimated, 2, cap), U256::from(11));
        assert_eq!(gas_price(estimated, 3, cap), U256::from(16));
        assert_eq!(gas_price(estimated, 4, cap), U256::from(25));
        assert_eq!(gas_price(estimated, 5, cap), U256::from(37));
        assert_eq!(gas_price(estimated, 6, cap), U256::from(50));
    }

    #[test]
    fn test_retry_with_gas_price_increase_once_until_cap_is_reached() {
        let mut contract = MockStableXContract::new();
        contract
            .expect_submit_solution()
            .times(1)
            .with(
                always(),
                always(),
                always(),
                eq(U256::from(DEFAULT_GAS_PRICE * 5)),
                eq(Some(2)),
                always(),
            )
            .return_once(|_, _, _, _, _, _| {
                async {
                    Err(MethodError::from_parts(
                    "submitSolution(uint32,uint256,address[],uint16[],uint128[],uint128[],uint16[])"
                        .to_owned(),
                    ExecutionError::ConfirmTimeout,
                ))
                }
                .boxed()
            });
        contract
            .expect_submit_solution()
            .with(
                always(),
                always(),
                always(),
                eq(U256::from(DEFAULT_GAS_PRICE * 7)),
                eq(None),
                always(),
            )
            .return_once(|_, _, _, _, _, _| async { Ok(()) }.boxed());

        let mut gas_station = MockGasPriceEstimating::new();
        gas_station.expect_estimate_gas_price().returning(|| {
            async {
                Ok(GasPrice {
                    fast: (DEFAULT_GAS_PRICE * 5).into(),
                    ..Default::default()
                })
            }
            .boxed()
        });

        retry_with_gas_price_increase(
            &contract,
            1,
            Solution::trivial(),
            1.into(),
            &gas_station,
            (DEFAULT_GAS_PRICE * 9).into(),
            U256::from(0),
        )
        .now_or_never()
        .unwrap()
        .unwrap();
    }

    #[test]
    fn test_retry_with_gas_price_respects_minimum_increase() {
        let mut contract = MockStableXContract::new();
        contract
            .expect_submit_solution()
            .times(1)
            .with(
                always(),
                always(),
                always(),
                eq(U256::from(DEFAULT_GAS_PRICE * 90)),
                always(),
                always(),
            )
            .return_once(|_, _, _, _, _, _| {
                async {
                    Err(MethodError::from_parts(
                    "submitSolution(uint32,uint256,address[],uint16[],uint128[],uint128[],uint16[])"
                        .to_owned(),
                    ExecutionError::ConfirmTimeout,
                ))
                }
                .boxed()
            });
        // There should not be a second call to submit_solution because 90 to 100 is not a large
        // enough gas price increase.

        let mut gas_station = MockGasPriceEstimating::new();
        gas_station.expect_estimate_gas_price().returning(|| {
            async {
                Ok(GasPrice {
                    fast: (DEFAULT_GAS_PRICE * 90).into(),
                    ..Default::default()
                })
            }
            .boxed()
        });

        assert!(retry_with_gas_price_increase(
            &contract,
            1,
            Solution::trivial(),
            1.into(),
            &gas_station,
            (DEFAULT_GAS_PRICE * 100).into(),
            0.into()
        )
        .now_or_never()
        .unwrap()
        .is_err());
    }

    #[test]
    fn test_retry_with_gas_price_increase_timeout() {
        let mut contract = MockStableXContract::new();
        contract
            .expect_submit_solution()
            .returning(|_, _, _, _, _, _| {
                async {
                    Err(MethodError::from_parts(
                    "submitSolution(uint32,uint256,address[],uint16[],uint128[],uint128[],uint16[])"
                        .to_owned(),
                    ExecutionError::ConfirmTimeout,
                ))
                }
                .boxed()
            });

        let mut gas_station = MockGasPriceEstimating::new();
        gas_station.expect_estimate_gas_price().returning(|| {
            async {
                Ok(GasPrice {
                    fast: (DEFAULT_GAS_PRICE * 5).into(),
                    ..Default::default()
                })
            }
            .boxed()
        });

        assert!(retry_with_gas_price_increase(
            &contract,
            1,
            Solution::trivial(),
            1.into(),
            &gas_station,
            (DEFAULT_GAS_PRICE * 15).into(),
            0.into()
        )
        .now_or_never()
        .unwrap()
        .is_err())
    }
}
