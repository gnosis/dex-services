//! Transactions with the same nonce must have a minimum gas price increase.

use futures::stream::{Stream, StreamExt as _};

/// openethereum requires that the gas price of the resubmitted transaction has increased by at
/// least 12.5%.
const MIN_GAS_PRICE_INCREASE_FACTOR: f64 = 1.125 * (1.0 + f64::EPSILON);

/// The minimum gas price that allows a new transaction to replace an older one.
pub fn minimum_increase(previous_gas_price: f64) -> f64 {
    (previous_gas_price * MIN_GAS_PRICE_INCREASE_FACTOR).ceil()
}

fn new_gas_price_estimate(
    previous_gas_price: f64,
    new_gas_price: f64,
    max_gas_price: f64,
) -> Option<f64> {
    let min_gas_price = minimum_increase(previous_gas_price);
    if min_gas_price > max_gas_price {
        return None;
    }
    if new_gas_price <= previous_gas_price {
        // Gas price has not increased.
        return None;
    }
    // Gas price could have increased but doesn't respect minimum increase so adjust it up.
    let new_price = min_gas_price.max(new_gas_price);
    Some(new_price.min(max_gas_price))
}

/// Adapt a stream of gas prices to only yield gas prices that respect the minimum gas price
/// increase while filtering out other values, including those over the cap.
pub fn enforce_minimum_increase_and_cap(
    gas_price_cap: f64,
    stream: impl Stream<Item = f64>,
) -> impl Stream<Item = f64> {
    let mut last_used_gas_price = 0.0;
    stream.filter_map(move |gas_price| {
        let gas_price = new_gas_price_estimate(last_used_gas_price, gas_price, gas_price_cap);
        if let Some(gas_price) = gas_price {
            last_used_gas_price = gas_price;
        }
        async move { gas_price }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::FutureExt;

    #[test]
    fn new_gas_price_estimate_() {
        // new below previous
        assert_eq!(new_gas_price_estimate(10.0, 0.0, 20.0), None);
        //new equal to previous
        assert_eq!(new_gas_price_estimate(10.0, 10.0, 20.0), None);
        // between previous and min increase rounded up to min increase
        assert_eq!(new_gas_price_estimate(10.0, 11.0, 20.0), Some(12.0));
        // between min increase and max stays same
        assert_eq!(new_gas_price_estimate(10.0, 13.0, 20.0), Some(13.0));
        // larger than max stays max
        assert_eq!(new_gas_price_estimate(10.0, 20.0, 20.0), Some(20.0));
        // cannot increase by min increase
        assert_eq!(new_gas_price_estimate(19.0, 18.0, 20.0), None);
        assert_eq!(new_gas_price_estimate(19.0, 19.0, 20.0), None);
        assert_eq!(new_gas_price_estimate(19.0, 19.5, 20.0), None);
        assert_eq!(new_gas_price_estimate(19.0, 20.0, 20.0), None);
        assert_eq!(new_gas_price_estimate(19.0, 25.0, 20.0), None);
    }

    #[test]
    #[allow(clippy::float_cmp)]
    fn stream_enforces_minimum_increase() {
        let input_stream = futures::stream::iter(vec![1.0, 1.0, 2.0, 2.5, 0.5]);
        let stream = enforce_minimum_increase_and_cap(2.0, input_stream);
        let result = stream.collect::<Vec<_>>().now_or_never().unwrap();
        assert_eq!(result, &[1.0, 2.0]);
    }
}
