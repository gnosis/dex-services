use crate::price_finding::error::PriceFindingError;
use crate::price_finding::PriceFinding;
use crate::util;
use dfusion_core::models::{AccountState, Order, Solution};
use std::cmp;
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
        Ok(ipairs(orders)
            .filter(|&(i, j)| {
                orders[i].buy_token == orders[j].sell_token
                    && orders[i].sell_token == orders[j].buy_token
            })
            .filter(|&(i, _)| orders[i].buy_token == 0 || orders[i].sell_token == 0)
            .filter_map(|(i, j)| compute_solution(orders, i, j, accounts))
            .nth(0)
            .unwrap_or_else(|| Solution::trivial(orders.len())))
    }
}

const ETH: u128 = 1_000_000_000_000_000_000u128;
const FEE_DENOM: u128 = 1000;

/// Returns an iterator over all unordered pairs of indices in slice.
fn ipairs<T>(slice: &[T]) -> impl Iterator<Item = (usize, usize)> {
    let len = slice.len();
    (0..len - 1)
        .map(move |i| (i + 1..len).map(move |j| (i, j)))
        .flatten()
}

/// Gets the limit price of the non-fee token for the given order considering
/// fees.
fn order_limit_price(order: &Order) -> u128 {
    use util::u128_to_u256 as u;

    let (fee, other, add_fees) = if order.buy_token == 0 {
        (order.buy_amount, order.sell_amount, true)
    } else {
        (order.sell_amount, order.buy_amount, false)
    };

    // upcast to 256 to avoid overflows
    let price = util::u256_to_u128((u(fee) * u(ETH)) / u(other));
    // account for fees in price
    if add_fees {
        (price * FEE_DENOM) / (FEE_DENOM - 1)
    } else {
        (price * (FEE_DENOM - 1)) / FEE_DENOM
    }
}

/// Attempts to compute a solution from two orders given the current accounts
/// state.
fn compute_solution(
    orders: &[Order],
    i: usize,
    j: usize,
    accounts: &AccountState,
) -> Option<Solution> {
    let price = cmp::min(
        order_limit_price(&orders[i]),
        order_limit_price(&orders[j]),
    );


    println!("{}", price);

    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use dfusion_core::models::account_state::test_util;
    use web3::types::Address;

    #[test]
    fn ipairs_for_slice() {
        assert_eq!(
            vec![(0, 1), (0, 2), (0, 3), (1, 2), (1, 3), (2, 3)],
            ipairs(&[0; 4]).collect::<Vec<_>>()
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
