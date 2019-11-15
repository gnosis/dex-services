use crate::price_finding::error::PriceFindingError;
use crate::price_finding::PriceFinding;
use crate::util;
use dfusion_core::models::{AccountState, Order, Solution};
use std::cmp::Ordering;

/// A naive solver for the stableX contract. It works almost identically to the
/// Snapp naive solver except it has a few extra requirements and it doesn't try
/// to be as optimal.
///
/// The solver works by finding the first pair of orders which matching sell and
/// buy tokens, where one of the traded tokens is the fee token, whose users have
/// enough balance to fully fill, and that has a viable solution.
///
/// The solution is computed by taking the price of the larger order and and
/// completely filling the smaller order. A large order is defined by having
/// more non-fee token volume. Account balances are checked for each solution.
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
            .filter(|&(i, j)| {
                has_enough_balance(&orders[i], accounts) && has_enough_balance(&orders[j], accounts)
            })
            .filter_map(|(i, j)| compute_solution(orders, i, j))
            .nth(0)
            .unwrap_or_else(|| Solution::trivial(orders.len())))
    }
}

const BASE_UNIT: u128 = 1_000_000_000_000_000_000u128;
const BASE_PRICE: u128 = BASE_UNIT;
const FEE_DENOM: u128 = 1000;

/// Returns an iterator over all unordered pairs of indices in slice.
fn ipairs<T>(slice: &[T]) -> impl Iterator<Item = (usize, usize)> {
    let len = slice.len();
    (0..len - 1)
        .map(move |i| (i + 1..len).map(move |j| (i, j)))
        .flatten()
}

/// Returns true if the owner of an order has enough balance to fully fill the
/// order; false otherwise.
fn has_enough_balance(order: &Order, accounts: &AccountState) -> bool {
    order.sell_amount <= accounts.read_balance(order.sell_token, order.account_id)
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
    let price = util::u256_to_u128((u(fee) * u(BASE_PRICE)) / u(other));
    // account for fees in price
    if add_fees {
        (price * FEE_DENOM) / (FEE_DENOM - 1)
    } else {
        (price * (FEE_DENOM - 1)) / FEE_DENOM
    }
}

/// Calculate the average of two values, note that this takes extra care not to
/// overflow.
fn average(a: u128, b: u128) -> u128 {
    let (min, max) = if a < b { (a, b) } else { (b, a) };
    min + ((max - min) / 2)
}

/// Calculate the executed buy amount from the executed sell amount and the buy
/// and sell prices of the traded tokens. This function returns a `None` if no
/// value can be found such that `executed_sell_amount(result, pb, ps) == xs`.
/// This function acts as an inverse to `executed_sell_amount`.
fn executed_buy_amount(xs: u128, pb: u128, ps: u128) -> Option<u128> {
    use util::u128_to_u256 as u;

    let v = util::u256_to_u128((((u(xs) * u(ps)) / u(FEE_DENOM)) * u(FEE_DENOM - 1)) / u(pb));

    // we need to account for rounding errors here, since this function is
    // essentially an inverse of `executed_sell_amount`; so find the maximum
    // error that `v` can have and find a value that works
    // TODO(nlordell): verify the maths

    macro_rules! return_if_correct {
        ($v:expr) => {
            if xs == executed_sell_amount($v, pb, ps) {
                return Some($v);
            }
        };
    }

    return_if_correct!(v);

    let delta = (ps / pb) + 1;
    for d in 1..=delta {
        return_if_correct!(v + d);
        return_if_correct!(v - d);
    }

    None
}

/// Calculate the executed sell amount from the executed buy amount and the buy
/// and sell prices of the traded tokens.
fn executed_sell_amount(xb: u128, pb: u128, ps: u128) -> u128 {
    use util::u128_to_u256 as u;

    util::u256_to_u128((((u(xb) * u(pb)) / u(FEE_DENOM - 1)) * u(FEE_DENOM)) / u(ps))
}

/// Attempts to compute a solution from two orders. Returns `None` if the order
/// limit prices cannot be satisfied or the executed buy amount for the order
/// buying the fee token cannot be calculated (due to rounding errors).
fn compute_solution(orders: &[Order], i: usize, j: usize) -> Option<Solution> {
    // here the terms buy and sell are relative to the non-fee token
    let (buy, sell) = if orders[i].buy_token != 0 {
        (i, j)
    } else {
        (j, i)
    };

    let buy_price = order_limit_price(&orders[buy]);
    let sell_price = order_limit_price(&orders[sell]);
    if buy_price < sell_price {
        // the order can't be filled since we are trying to buy at a lower price
        // than is actually being sold
        return None;
    }

    // let the larger order set the price
    let (price, exec_amt) = match orders[buy].buy_amount.cmp(&orders[sell].sell_amount) {
        Ordering::Greater => (buy_price, orders[sell].sell_amount),
        Ordering::Equal => (average(buy_price, sell_price), orders[buy].buy_amount),
        Ordering::Less => (sell_price, orders[buy].buy_amount),
    };

    // when considering only two orders, a solution has exactly two degrees of
    // freedom, the amount being traded and the price of the non-fee token; we
    // have chosen both values so now all we need to do is calculate the other
    // values required for the solution
    let mut solution = Solution::trivial(orders.len());

    solution.prices[0] = BASE_PRICE;
    solution.prices[orders[buy].buy_token as usize] = price;
    solution.executed_buy_amounts[buy] = exec_amt;
    solution.executed_buy_amounts[sell] = executed_buy_amount(exec_amt, BASE_PRICE, price)?;
    solution.executed_sell_amounts[buy] = executed_sell_amount(exec_amt, price, BASE_PRICE);
    solution.executed_sell_amounts[sell] = exec_amt;

    Some(solution)
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
        let users = [Address::from(0), Address::from(1)];
        let accounts = {
            let mut accounts = AccountState::default();
            accounts.num_tokens = u16::max_value();
            accounts.increment_balance(0, users[0], 3000 * BASE_UNIT);
            accounts.increment_balance(1, users[1], 3000 * BASE_UNIT);
            accounts
        };
        let orders = vec![
            Order {
                batch_information: None,
                account_id: users[0],
                sell_token: 0,
                buy_token: 1,
                sell_amount: 2000 * BASE_UNIT,
                buy_amount: 999 * BASE_UNIT,
            },
            Order {
                batch_information: None,
                account_id: users[1],
                sell_token: 1,
                buy_token: 0,
                sell_amount: 999 * BASE_UNIT,
                buy_amount: 1996 * BASE_UNIT,
            },
        ];

        let solution = StableXNaiveSolver.find_prices(&orders, &accounts).unwrap();
        assert!(solution.is_non_trivial());
    }

    #[test]
    fn basic_trade() {
        let orders = vec![
            Order {
                batch_information: None,
                account_id: Address::from(0),
                sell_token: 0,
                buy_token: 1,
                sell_amount: 20_020_020_020_021_019_020,
                buy_amount: BASE_UNIT,
            },
            Order {
                batch_information: None,
                account_id: Address::from(1),
                sell_token: 1,
                buy_token: 0,
                sell_amount: BASE_UNIT,
                buy_amount: 19_979_999_999_999_001_000,
            },
        ];
        let accounts = test_util::create_account_state_with_balance_for(&orders);

        let solution = StableXNaiveSolver.find_prices(&orders, &accounts).unwrap();
        assert!(solution.is_non_trivial());
    }
}
