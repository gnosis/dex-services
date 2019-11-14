use crate::price_finding::error::PriceFindingError;
use crate::price_finding::PriceFinding;
use crate::util;
use dfusion_core::models::{AccountState, Order, Solution};
use web3::types::U256;

/// A naive solver for the stableX contract. It works almost identically to the
/// Snapp naive solver except it has a few extra requirements and it doesn't try
/// to be as optimal.
///
/// The solver works by finding the first pair of orders which matching sell and
/// buy tokens, where one of the traded tokens is the fee token, that has a
/// viable solution.
///
/// The solution is computed by taking the price of the larger order and and
/// completely filling the smaller order. A large order is defined by having
/// more fee token volume.
pub struct StableXNaiveSolver;

impl PriceFinding for StableXNaiveSolver {
    fn find_prices(
        &self,
        orders: &[Order],
        accounts: &AccountState,
    ) -> Result<Solution, PriceFindingError> {
        Ok(pairs(orders)
            .filter(|(x, y)| x.buy_token == y.sell_token && x.sell_token == y.buy_token)
            .filter(|(x, _)| x.buy_token == 0 || x.sell_token == 0)
            .filter_map(|(x, y)| compute_solution(x, y, accounts))
            .nth(0)
            .unwrap_or_else(|| Solution::trivial(orders.len())))
    }
}

const ETH: u128 = 1_000_000_000_000_000_000u128;
const FEE_DENOM: u128 = 1000;

/// Returns an iterator over all unordered pairs in slice.
fn pairs<'a, T>(slice: &'a [T]) -> impl Iterator<Item = (&'a T, &'a T)> + 'a {
    (0..slice.len() - 1)
        .map(move |i| (i + 1..slice.len()).map(move |j| (i, j)))
        .flatten()
        .map(move |(i, j)| (&slice[i], &slice[j]))
}

/// Orders and returns the orders in ascending order.
fn ordered_orders<'a>(x: &'a Order, y: &'a Order) -> (&'a Order, &'a Order) {
    let x_is_larger = if x.buy_token == 0 {
        x.buy_amount > y.sell_amount
    } else {
        x.sell_amount > y.buy_amount
    };
    if x_is_larger {
        (y, x)
    } else {
        (x, y)
    }
}

/// Gets the limit price of the non-fee token for the given order
fn order_limit_price(order: &Order) -> u128 {
    use util::u128_to_u256 as u;

    let (fee, other) = if order.buy_token == 0 {
        (order.buy_amount, order.sell_amount)
    } else {
        (order.sell_amount, order.buy_amount)
    };

    // upcast to 256 to avoid overflows
    util::u256_to_u128((u(fee) * u(ETH)) / u(other))
}

/// Attempts to compute a solution from two orders given the current accounts
/// state.
fn compute_solution(x: &Order, y: &Order, accounts: &AccountState) -> Option<Solution> {
    let (small, large) = ordered_orders(x, y);
    let price = order_limit_price(large);

    println!("price: {}", price);

    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use dfusion_core::models::account_state::test_util;
    use web3::types::Address;

    #[test]
    fn int_pairs() {
        assert_eq!(
            vec![(0, 1), (0, 2), (0, 3), (1, 2), (1, 3), (2, 3)],
            pairs(&[0, 1, 2, 3])
                .map(|(x, y)| (*x, *y))
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn stablex_e2e_auction() {
        let orders = vec![
            Order {
                batch_information: None,
                account_id: Address::from(0),
                sell_token: 0,
                buy_token: 1,
                sell_amount: 2000 * ETH,
                buy_amount: 999 * ETH,
            },
            Order {
                batch_information: None,
                account_id: Address::from(1),
                sell_token: 1,
                buy_token: 0,
                sell_amount: 999 * ETH,
                buy_amount: 1996 * ETH,
            },
        ];
        let accounts = test_util::create_account_state_with_balance_for(&orders);

        let solution = StableXNaiveSolver.find_prices(&orders, &accounts).unwrap();
        assert!(false);
    }
}
