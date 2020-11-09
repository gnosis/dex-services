use super::GasPriceEstimating;
use anyhow::{anyhow, Result};
use std::{future::Future, time::Duration};

// Uses the first successful estimator.
pub struct PriorityGasPrice {
    estimators: Vec<Box<dyn GasPriceEstimating>>,
}

impl PriorityGasPrice {
    pub fn new(estimators: Vec<Box<dyn GasPriceEstimating>>) -> Self {
        Self { estimators }
    }

    async fn prioritize<'a, T, F>(&'a self, operation: T) -> Result<f64>
    where
        T: Fn(&'a dyn GasPriceEstimating) -> F,
        F: Future<Output = Result<f64>>,
    {
        for estimator in &self.estimators {
            match operation(estimator.as_ref()).await {
                Ok(result) => return Ok(result),
                Err(err) => log::error!("gas estimator failed: {:?}", err),
            }
        }
        Err(anyhow!("all gas estimators failed"))
    }
}

#[async_trait::async_trait]
impl GasPriceEstimating for PriorityGasPrice {
    async fn estimate_with_limits(&self, gas_limit: f64, time_limit: Duration) -> Result<f64> {
        self.prioritize(|estimator| estimator.estimate_with_limits(gas_limit, time_limit))
            .await
    }

    async fn estimate(&self) -> Result<f64> {
        self.prioritize(|estimator| estimator.estimate()).await
    }
}

#[cfg(test)]
mod tests {
    use super::super::MockGasPriceEstimating;
    use super::*;
    use assert_approx_eq::assert_approx_eq;
    use futures::future::FutureExt;

    #[test]
    fn prioritize_picks_first() {
        let mut estimator_0 = MockGasPriceEstimating::new();
        let estimator_1 = MockGasPriceEstimating::new();

        estimator_0.expect_estimate().times(1).returning(|| Ok(1.0));

        let priority = PriorityGasPrice::new(vec![Box::new(estimator_0), Box::new(estimator_1)]);
        let result = priority.estimate().now_or_never().unwrap().unwrap();
        assert_approx_eq!(result, 1.0);
    }

    #[test]
    fn prioritize_picks_second() {
        let mut estimator_0 = MockGasPriceEstimating::new();
        let mut estimator_1 = MockGasPriceEstimating::new();

        estimator_0
            .expect_estimate()
            .times(1)
            .returning(|| Err(anyhow!("")));
        estimator_1.expect_estimate().times(1).returning(|| Ok(2.0));

        let priority = PriorityGasPrice::new(vec![Box::new(estimator_0), Box::new(estimator_1)]);
        let result = priority.estimate().now_or_never().unwrap().unwrap();
        assert_approx_eq!(result, 2.0);
    }
}
