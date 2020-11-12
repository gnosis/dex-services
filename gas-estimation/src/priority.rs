use super::GasPriceEstimating;
use anyhow::{anyhow, Result};
use std::{
    future::Future,
    sync::atomic::{AtomicUsize, Ordering},
    time::Duration,
};

// Errors of an individual estimator are logged as warnings until it has failed this many times in
// a row at which point they are logged as errors.
// This is useful to reduce alerts for estimators that sometimes fail individual requests while
// still getting them when the estimator really goes down.
const LOG_ERROR_AFTER_N_ERRORS: usize = 10;

// Uses the first successful estimator.
pub struct PriorityGasPriceEstimating {
    estimators: Vec<Estimator>,
}

struct Estimator {
    estimator: Box<dyn GasPriceEstimating>,
    errors_in_a_row: AtomicUsize,
}

impl PriorityGasPriceEstimating {
    pub fn new(estimators: Vec<Box<dyn GasPriceEstimating>>) -> Self {
        let estimators = estimators
            .into_iter()
            .map(|estimator| Estimator {
                estimator,
                errors_in_a_row: AtomicUsize::new(0),
            })
            .collect();
        Self { estimators }
    }

    async fn prioritize<'a, T, F>(&'a self, operation: T) -> Result<f64>
    where
        T: Fn(&'a dyn GasPriceEstimating) -> F,
        F: Future<Output = Result<f64>>,
    {
        for (i, estimator) in self.estimators.iter().enumerate() {
            match operation(estimator.estimator.as_ref()).await {
                Ok(result) => {
                    estimator.errors_in_a_row.store(0, Ordering::SeqCst);
                    return Ok(result);
                }
                Err(err) => {
                    let num_errors = estimator.errors_in_a_row.fetch_add(1, Ordering::SeqCst) + 1;
                    if num_errors < LOG_ERROR_AFTER_N_ERRORS {
                        log::warn!("gas estimator {} failed: {:?}", i, err);
                    } else {
                        log::error!("gas estimator {} failed: {:?}", i, err);
                    }
                }
            }
        }
        Err(anyhow!("all gas estimators failed"))
    }
}

#[async_trait::async_trait]
impl GasPriceEstimating for PriorityGasPriceEstimating {
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
    fn prioritize_picks_first_if_first_succeeds() {
        let mut estimator_0 = MockGasPriceEstimating::new();
        let estimator_1 = MockGasPriceEstimating::new();

        estimator_0.expect_estimate().times(1).returning(|| Ok(1.0));

        let priority =
            PriorityGasPriceEstimating::new(vec![Box::new(estimator_0), Box::new(estimator_1)]);
        let result = priority.estimate().now_or_never().unwrap().unwrap();
        assert_approx_eq!(result, 1.0);
    }

    #[test]
    fn prioritize_picks_second_if_first_fails() {
        let mut estimator_0 = MockGasPriceEstimating::new();
        let mut estimator_1 = MockGasPriceEstimating::new();

        estimator_0
            .expect_estimate()
            .times(1)
            .returning(|| Err(anyhow!("")));
        estimator_1.expect_estimate().times(1).returning(|| Ok(2.0));

        let priority =
            PriorityGasPriceEstimating::new(vec![Box::new(estimator_0), Box::new(estimator_1)]);
        let result = priority.estimate().now_or_never().unwrap().unwrap();
        assert_approx_eq!(result, 2.0);
    }

    #[test]
    fn prioritize_fails_if_all_fail() {
        let mut estimator_0 = MockGasPriceEstimating::new();
        let mut estimator_1 = MockGasPriceEstimating::new();

        estimator_0
            .expect_estimate()
            .times(1)
            .returning(|| Err(anyhow!("")));
        estimator_1
            .expect_estimate()
            .times(1)
            .returning(|| Err(anyhow!("")));

        let priority =
            PriorityGasPriceEstimating::new(vec![Box::new(estimator_0), Box::new(estimator_1)]);
        let result = priority.estimate().now_or_never().unwrap();
        assert!(result.is_err());
    }
}
