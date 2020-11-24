use crate::{contracts::stablex_contract::SOLUTION_SUBMISSION_GAS_LIMIT, util::AsyncSleeping};
use futures::stream::{self, Stream, StreamExt as _};
use gas_estimation::GasPriceEstimating;
use std::time::{Duration, Instant};
use transaction_retry::gas_price_increase;

const GAS_PRICE_REFRESH_INTERVAL: Duration = Duration::from_secs(15);

/// Create a never ending stream of gas prices based on checking the estimator in fixed intervals
/// and enforcing the minimum increase. Errors are ignored.
pub fn gas_price_stream<'a>(
    target_confirm_time: Instant,
    gas_price_cap: f64,
    estimator: &'a dyn GasPriceEstimating,
    sleep: &'a dyn AsyncSleeping,
) -> impl Stream<Item = f64> + 'a {
    let stream = stream::unfold(true, move |first_call| async move {
        if !first_call {
            sleep.sleep(GAS_PRICE_REFRESH_INTERVAL).await;
        }
        let time_remaining = target_confirm_time.saturating_duration_since(Instant::now());
        let estimate = estimator
            .estimate_with_limits(SOLUTION_SUBMISSION_GAS_LIMIT as f64, time_remaining)
            .await;
        Some((estimate, false))
    })
    .filter_map(|gas_price_result| async move {
        match gas_price_result {
            Ok(gas_price) => {
                log::debug!("estimated gas price {}", gas_price);
                Some(gas_price)
            }
            Err(err) => {
                log::error!("gas price estimation failed: {:?}", err);
                None
            }
        }
    });
    gas_price_increase::enforce_minimum_increase_and_cap(gas_price_cap, stream)
}
