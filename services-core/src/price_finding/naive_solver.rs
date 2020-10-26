use crate::models::{AccountState, ExecutedOrder, Order, Solution};
use crate::price_finding::price_finder_interface::{Fee, PriceFinding};
use crate::util::{CeiledDiv, CheckedConvertU128};

use std::collections::HashMap;
use std::time::Duration;

use anyhow::Result;
use ethcontract::U256;

const BASE_UNIT: u128 = 1_000_000_000_000_000_000u128;
const BASE_PRICE: u128 = BASE_UNIT;

pub enum OrderPairType {
    LhsFullyFilled,
    RhsFullyFilled,
    BothFullyFilled,
}

trait Matchable {
    /// Returns whether the orders can be matched. For this the tokens need
    /// to match, there must be a price that satisfies both orders and if there
    /// is a fee then one of the tokens must be the fee token.
    fn attracts(&self, other: &Order, fee: &Option<Fee>) -> bool;
    /// Returns whether the account to which the order belongs has at least
    /// as many funds of the sell token as the order's sell amount.
    fn sufficient_seller_funds(&self, state: &AccountState) -> bool;
    fn match_compare(
        &self,
        other: &Order,
        state: &AccountState,
        fee: &Option<Fee>,
    ) -> Option<OrderPairType>;
    /// Returns whether this order's sell token is the other order's buy token
    /// and vice versa.
    fn opposite_tokens(&self, other: &Order) -> bool;
    /// Returns whether there is a price that satisfies both orders.
    fn have_price_overlap(&self, other: &Order) -> bool;
    /// Returns whether the sell or buy token is the fee.
    fn trades_fee_token(&self, fee: &Fee) -> bool;
}

impl Matchable for Order {
    fn attracts(&self, other: &Order, fee: &Option<Fee>) -> bool {
        // We can only match orders that touch the fee token
        if fee.is_some()
            && ![self.sell_token, self.buy_token].contains(&fee.as_ref().unwrap().token)
        {
            return false;
        }
        self.opposite_tokens(other) && self.have_price_overlap(other)
    }

    fn sufficient_seller_funds(&self, state: &AccountState) -> bool {
        state.read_balance(self.sell_token, self.account_id)
            >= U256::from(self.remaining_sell_amount)
    }

    fn match_compare(
        &self,
        other: &Order,
        state: &AccountState,
        fee: &Option<Fee>,
    ) -> Option<OrderPairType> {
        if !self.sufficient_seller_funds(&state)
            || !other.sufficient_seller_funds(&state)
            || !self.attracts(other, fee)
            || !fee
                .as_ref()
                .map(|fee| self.trades_fee_token(fee))
                .unwrap_or(true)
        {
            return None;
        }

        if self.numerator <= other.denominator && self.denominator <= other.numerator {
            Some(OrderPairType::LhsFullyFilled)
        } else if self.numerator >= other.denominator && self.denominator >= other.numerator {
            Some(OrderPairType::RhsFullyFilled)
        } else {
            Some(OrderPairType::BothFullyFilled)
        }
    }

    fn opposite_tokens(&self, other: &Order) -> bool {
        self.buy_token == other.sell_token && self.sell_token == other.buy_token
    }

    fn have_price_overlap(&self, other: &Order) -> bool {
        self.denominator > 0
            && other.denominator > 0
            && U256::from(self.numerator) * U256::from(other.numerator)
                <= U256::from(other.denominator) * U256::from(self.denominator)
    }

    fn trades_fee_token(&self, fee: &Fee) -> bool {
        self.buy_token == fee.token || self.sell_token == fee.token
    }
}

/// Implements PriceFinding in a simplistic way.
///
/// Tries to find a match of two orders that trade the fee token and uses this
/// as the only trade in the solution.
/// If no such match can be found then the trivial solution is returned.
pub struct NaiveSolver {
    fee: Option<Fee>,
}

impl NaiveSolver {
    pub fn new(fee: Option<Fee>) -> Self {
        NaiveSolver { fee }
    }
}

struct Match {
    order_pair_type: OrderPairType,
    orders: OrderPair,
}

type PriceMap = HashMap<u16, u128>;
type OrderPair = [Order; 2];
type ExecutedOrderPair = [ExecutedOrder; 2];

#[async_trait::async_trait]
impl PriceFinding for NaiveSolver {
    async fn find_prices(
        &self,
        orders: &[Order],
        state: &AccountState,
        _: Duration,
        _: u128,
    ) -> Result<Solution> {
        // Convert orders into the form where they have remaining == denominator.
        let orders = orders
            .iter()
            .map(|order| {
                let (buy_amount, sell_amount) = order.compute_remaining_buy_sell_amounts();
                let mut order = order.clone();
                order.numerator = buy_amount;
                order.denominator = sell_amount;
                order.remaining_sell_amount = sell_amount;
                order
            })
            .collect::<Vec<_>>();
        let solution = if let Some(first_match) = find_first_match(&orders, state, &self.fee) {
            let (executed_orders, prices) = create_executed_orders(&first_match, &self.fee);
            if let Some(ref fee) = self.fee {
                create_solution_with_fee(&first_match.orders, fee, executed_orders, prices)
            } else {
                Solution {
                    prices,
                    executed_orders: executed_orders.to_vec(),
                }
            }
        } else {
            Solution::trivial()
        };
        Ok(solution)
    }
}

fn find_first_match(orders: &[Order], state: &AccountState, fee: &Option<Fee>) -> Option<Match> {
    for (i, x) in orders.iter().enumerate() {
        for y in orders.iter().skip(i + 1) {
            if let Some(order_pair_type) = x.match_compare(&y, &state, fee) {
                return Some(Match {
                    order_pair_type,
                    orders: [x.clone(), y.clone()],
                });
            }
        }
    }
    None
}

fn create_executed_orders(first_match: &Match, fee: &Option<Fee>) -> (ExecutedOrderPair, PriceMap) {
    fn create_executed_order(order: &Order, sell_amount: u128, buy_amount: u128) -> ExecutedOrder {
        ExecutedOrder {
            account_id: order.account_id,
            order_id: order.id,
            buy_amount,
            sell_amount,
        }
    }

    // Preprocess order to leave "space" for fee to be taken
    let x = order_with_buffer_for_fee(&first_match.orders[0], fee);
    let y = order_with_buffer_for_fee(&first_match.orders[1], fee);

    let create_orders = |x_sell_amount, x_buy_amount, y_sell_amount, y_buy_amount| {
        [
            create_executed_order(&x, x_sell_amount, x_buy_amount),
            create_executed_order(&y, y_sell_amount, y_buy_amount),
        ]
    };

    let mut prices = HashMap::new();
    let executed_orders = match first_match.order_pair_type {
        OrderPairType::LhsFullyFilled => {
            prices.insert(x.buy_token, x.denominator);
            prices.insert(y.buy_token, x.numerator);
            create_orders(x.denominator, x.numerator, x.numerator, x.denominator)
        }
        OrderPairType::RhsFullyFilled => {
            prices.insert(x.sell_token, y.denominator);
            prices.insert(y.sell_token, y.numerator);
            create_orders(y.numerator, y.denominator, y.denominator, y.numerator)
        }
        OrderPairType::BothFullyFilled => {
            prices.insert(y.buy_token, y.denominator);
            prices.insert(x.buy_token, x.denominator);
            create_orders(x.denominator, y.denominator, y.denominator, x.denominator)
        }
    };

    (executed_orders, prices)
}

fn create_solution_with_fee(
    orders: &OrderPair,
    fee: &Fee,
    mut executed_orders: ExecutedOrderPair,
    mut prices: PriceMap,
) -> Solution {
    // normalize prices so fee token price is BASE_PRICE
    let pre_normalized_fee_price = prices.get(&fee.token).copied().unwrap_or(0);
    if pre_normalized_fee_price == 0 {
        return Solution::trivial();
    }
    for price in prices.values_mut() {
        *price = match normalize_price(*price, pre_normalized_fee_price) {
            Some(price) => price,
            None => return Solution::trivial(),
        };
    }

    // apply fee to volumes account for rounding errors, moving them to
    // the fee token
    for (i, order) in orders.iter().enumerate() {
        let executed_order = &mut executed_orders[i];
        if order.sell_token == fee.token {
            let price_buy = prices[&order.buy_token];
            executed_order.sell_amount =
                executed_sell_amount(fee, executed_order.buy_amount, price_buy, BASE_PRICE);
        } else {
            let price_sell = prices[&order.sell_token];
            executed_order.buy_amount = match executed_buy_amount(
                fee,
                executed_order.sell_amount,
                BASE_PRICE,
                price_sell,
            ) {
                Some(exec_buy_amt) => exec_buy_amt,
                None => return Solution::trivial(),
            };
        }
    }

    Solution {
        prices,
        executed_orders: executed_orders.to_vec(),
    }
}

fn order_with_buffer_for_fee(order: &Order, fee: &Option<Fee>) -> Order {
    match fee {
        Some(fee) => {
            let mut order = order.clone();
            // In order to make space for a fee in the existing limit, we need to
            // a) receive more stuff (while giving away the same)
            // b) give away less stuff (while receiving the same)
            let fee_denominator = (1.0 / fee.ratio) as u128;
            if fee.token == order.buy_token {
                order.numerator =
                    (order.numerator * fee_denominator).ceiled_div(fee_denominator - 1)
            } else if fee.token == order.sell_token {
                order.denominator = order.denominator * (fee_denominator - 1) / fee_denominator
            }
            order
        }
        None => order.clone(),
    }
}

/// Normalizes a price base on the pre-normalized fee price.
fn normalize_price(price: u128, pre_normalized_fee_price: u128) -> Option<u128> {
    // upcast to u256 to avoid overflows
    (U256::from(price) * U256::from(BASE_PRICE))
        .ceiled_div(U256::from(pre_normalized_fee_price))
        .as_u128_checked()
}

/// Calculate the executed sell amount from the fee, executed buy amount, and
/// the buy and sell prices of the traded tokens.
fn executed_sell_amount(fee: &Fee, exec_buy_amt: u128, buy_price: u128, sell_price: u128) -> u128 {
    let fee_denominator = (1.0 / fee.ratio) as u128;
    ((((U256::from(exec_buy_amt) * U256::from(buy_price)) / U256::from(fee_denominator - 1))
        * U256::from(fee_denominator))
        / U256::from(sell_price))
    .as_u128()
}

/// Calculate the executed buy amount from the fee, executed sell amount, and
/// the buy and sell prices of the traded tokens. This function returns a `None`
/// if no value can be found such that `executed_sell_amount(result, pb, ps) ==
/// xs`. This function acts as an inverse to `executed_sell_amount`.
fn executed_buy_amount(
    fee: &Fee,
    exec_sell_amt: u128,
    buy_price: u128,
    sell_price: u128,
) -> Option<u128> {
    let fee_denominator = (1.0 / fee.ratio) as u128;
    let exec_buy_amt = (((U256::from(exec_sell_amt) * U256::from(sell_price))
        / U256::from(fee_denominator))
        * U256::from(fee_denominator - 1))
    .ceiled_div(U256::from(buy_price))
    .as_u128();

    // we need to account for rounding errors here, since this function is
    // essentially an inverse of `executed_sell_amount`; when the buy price is
    // higher than the sell price, there are executed sell amounts that cannot
    // be satisfied, check the executed buy amount correctly "round trips" to
    // the specified executed sell amount and return `None` if it doesn't
    if exec_sell_amt == executed_sell_amount(fee, exec_buy_amt, buy_price, sell_price) {
        Some(exec_buy_amt)
    } else {
        None
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use crate::models::order::test_util::order_to_executed_order;
    use crate::models::AccountState;
    use ethcontract::{Address, U256};
    use futures::FutureExt as _;
    use std::collections::HashMap;

    #[test]
    fn test_type_left_fully_matched_no_fee() {
        let orders = order_pair_first_fully_matching_second();
        let state = AccountState::with_balance_for(&orders);

        let solver = NaiveSolver::new(None);
        let res = solver
            .find_prices(&orders, &state, Duration::default(), 0)
            .now_or_never()
            .unwrap()
            .unwrap();

        assert_eq!(
            vec![
                order_to_executed_order(&orders[0], 52 * BASE_UNIT, 4 * BASE_UNIT),
                order_to_executed_order(&orders[1], 4 * BASE_UNIT, 52 * BASE_UNIT),
            ],
            res.executed_orders
        );
        assert_eq!(4 * BASE_UNIT, res.prices[&0]);
        assert_eq!(52 * BASE_UNIT, res.prices[&1]);

        check_solution(&orders, res, &None).unwrap();
    }

    #[test]
    fn test_type_left_fully_matched_with_fee() {
        let orders = order_pair_first_fully_matching_second();
        let state = AccountState::with_balance_for(&orders);
        let fee = Some(Fee {
            token: 0,
            ratio: 0.001,
        });

        let solver = NaiveSolver::new(fee.clone());
        let res = solver
            .find_prices(&orders, &state, Duration::default(), 0)
            .now_or_never()
            .unwrap()
            .unwrap();
        check_solution(&orders, res, &fee).unwrap();
    }

    #[test]
    fn test_type_right_fully_matched_no_fee() {
        let mut orders = order_pair_first_fully_matching_second();
        orders.reverse();
        let state = AccountState::with_balance_for(&orders);

        let solver = NaiveSolver::new(None);
        let res = solver
            .find_prices(&orders, &state, Duration::default(), 0)
            .now_or_never()
            .unwrap()
            .unwrap();

        assert_eq!(
            vec![
                order_to_executed_order(&orders[0], 4 * BASE_UNIT, 52 * BASE_UNIT),
                order_to_executed_order(&orders[1], 52 * BASE_UNIT, 4 * BASE_UNIT),
            ],
            res.executed_orders
        );
        assert_eq!(4 * BASE_UNIT, res.prices[&0]);
        assert_eq!(52 * BASE_UNIT, res.prices[&1]);

        check_solution(&orders, res, &None).unwrap();
    }

    #[test]
    fn test_type_right_fully_matched_with_fee() {
        let mut orders = order_pair_first_fully_matching_second();
        orders.reverse();
        let state = AccountState::with_balance_for(&orders);
        let fee = Some(Fee {
            token: 0,
            ratio: 0.001,
        });

        let solver = NaiveSolver::new(fee.clone());
        let res = solver
            .find_prices(&orders, &state, Duration::default(), 0)
            .now_or_never()
            .unwrap()
            .unwrap();
        check_solution(&orders, res, &fee).unwrap();
    }

    #[test]
    fn test_type_both_fully_matched_no_fee() {
        let orders = order_pair_both_fully_matched();
        let state = AccountState::with_balance_for(&orders);

        let solver = NaiveSolver::new(None);
        let res = solver
            .find_prices(&orders, &state, Duration::default(), 0)
            .now_or_never()
            .unwrap()
            .unwrap();

        assert_eq!(
            vec![
                order_to_executed_order(&orders[0], 10 * BASE_UNIT, 16 * BASE_UNIT),
                order_to_executed_order(&orders[1], 16 * BASE_UNIT, 10 * BASE_UNIT),
            ],
            res.executed_orders
        );
        assert_eq!(10 * BASE_UNIT, res.prices[&1]);
        assert_eq!(16 * BASE_UNIT, res.prices[&2]);

        check_solution(&orders, res, &None).unwrap();
    }

    #[test]
    fn test_type_both_fully_matched_with_fee() {
        let orders = order_pair_both_fully_matched();
        let state = AccountState::with_balance_for(&orders);
        let fee = Some(Fee {
            token: 2,
            ratio: 0.001,
        });
        let solver = NaiveSolver::new(fee.clone());
        let res = solver
            .find_prices(&orders, &state, Duration::default(), 0)
            .now_or_never()
            .unwrap()
            .unwrap();
        check_solution(&orders, res, &fee).unwrap();
    }

    #[test]
    fn test_retreth_example() {
        let orders = vec![
            Order {
                id: 0,
                account_id: Address::from_low_u64_be(0),
                sell_token: 3,
                buy_token: 2,
                denominator: 12,
                numerator: 12,
                remaining_sell_amount: 12,
                valid_from: 0,
                valid_until: 0,
            },
            Order {
                id: 0,
                account_id: Address::from_low_u64_be(1),
                sell_token: 2,
                buy_token: 3,
                denominator: 20,
                numerator: 22,
                remaining_sell_amount: 20,
                valid_from: 0,
                valid_until: 0,
            },
            Order {
                id: 0,
                account_id: Address::from_low_u64_be(2),
                sell_token: 3,
                buy_token: 1,
                denominator: 10,
                numerator: 150,
                remaining_sell_amount: 10,
                valid_from: 0,
                valid_until: 0,
            },
            Order {
                id: 0,
                account_id: Address::from_low_u64_be(3),
                sell_token: 2,
                buy_token: 1,
                denominator: 15,
                numerator: 180,
                remaining_sell_amount: 15,
                valid_from: 0,
                valid_until: 0,
            },
            Order {
                id: 0,
                account_id: Address::from_low_u64_be(4),
                sell_token: 1,
                buy_token: 2,
                denominator: 52,
                numerator: 4,
                remaining_sell_amount: 52,
                valid_from: 0,
                valid_until: 0,
            },
            Order {
                id: 0,
                account_id: Address::from_low_u64_be(5),
                sell_token: 1,
                buy_token: 3,
                denominator: 280,
                numerator: 20,
                remaining_sell_amount: 280,
                valid_from: 0,
                valid_until: 0,
            },
        ];
        let state = AccountState::with_balance_for(&orders);

        let solver = NaiveSolver::new(None);
        let res = solver
            .find_prices(&orders, &state, Duration::default(), 0)
            .now_or_never()
            .unwrap()
            .unwrap();

        assert_eq!(
            vec![
                order_to_executed_order(&orders[3], 4, 52),
                order_to_executed_order(&orders[4], 52, 4),
            ],
            res.executed_orders
        );
        assert_eq!(4, res.prices[&1]);
        assert_eq!(52, res.prices[&2]);

        check_solution(&orders, res, &None).unwrap();
    }

    #[test]
    fn test_insufficient_balance() {
        const NUM_TOKENS: u16 = 10;
        let state = AccountState::new(vec![0; (NUM_TOKENS * 2) as usize], NUM_TOKENS);
        let orders = vec![
            Order {
                id: 0,
                account_id: Address::from_low_u64_be(1),
                sell_token: 1,
                buy_token: 2,
                denominator: 52,
                numerator: 4,
                remaining_sell_amount: 52,
                valid_from: 0,
                valid_until: 0,
            },
            Order {
                id: 0,
                account_id: Address::from_low_u64_be(0),
                sell_token: 2,
                buy_token: 1,
                denominator: 15,
                numerator: 180,
                remaining_sell_amount: 15,
                valid_from: 0,
                valid_until: 0,
            },
        ];

        let solver = NaiveSolver::new(None);
        let res = solver
            .find_prices(&orders, &state, Duration::default(), 0)
            .now_or_never()
            .unwrap()
            .unwrap();
        assert!(!res.is_non_trivial());
    }

    #[test]
    fn test_no_matches() {
        let orders = vec![
            Order {
                id: 0,
                account_id: Address::from_low_u64_be(1),
                sell_token: 1,
                buy_token: 2,
                denominator: 52,
                numerator: 4,
                remaining_sell_amount: 52,
                valid_from: 0,
                valid_until: 0,
            },
            Order {
                id: 0,
                account_id: Address::from_low_u64_be(0),
                sell_token: 2,
                buy_token: 1,
                denominator: 10,
                numerator: 180,
                remaining_sell_amount: 10,
                valid_from: 0,
                valid_until: 0,
            },
        ];
        let state = AccountState::with_balance_for(&orders);

        let solver = NaiveSolver::new(None);
        let res = solver
            .find_prices(&orders, &state, Duration::default(), 0)
            .now_or_never()
            .unwrap()
            .unwrap();
        assert!(!res.is_non_trivial());
    }

    #[test]
    fn test_stablex_contract_example() {
        let orders = vec![
            Order {
                id: 0,
                account_id: Address::from_low_u64_be(0),
                sell_token: 0,
                buy_token: 1,
                denominator: 20000,
                numerator: 9990,
                remaining_sell_amount: 20000,
                valid_from: 0,
                valid_until: 0,
            },
            Order {
                id: 1,
                account_id: Address::from_low_u64_be(0),
                sell_token: 1,
                buy_token: 0,
                denominator: 9990,
                numerator: 19960,
                remaining_sell_amount: 9990,
                valid_from: 0,
                valid_until: 0,
            },
        ];
        let state = AccountState::with_balance_for(&orders);

        let fee = Some(Fee {
            token: 0,
            ratio: 0.001,
        });
        let solver = NaiveSolver::new(fee.clone());
        let res = solver
            .find_prices(&orders, &state, Duration::default(), 0)
            .now_or_never()
            .unwrap()
            .unwrap();

        assert_eq!(
            vec![
                order_to_executed_order(&orders[0], 20000, 9990),
                order_to_executed_order(&orders[1], 9990, 19961)
            ],
            res.executed_orders
        );
        assert_eq!(res.prices[&0], BASE_PRICE);
        assert_eq!(res.prices[&1], BASE_PRICE * 2);

        check_solution(&orders, res, &fee).unwrap();
    }

    #[test]
    fn stablex_e2e_auction() {
        let users = [Address::from_low_u64_be(0), Address::from_low_u64_be(1)];
        let state = {
            let mut state = AccountState::default();
            state.increase_balance(users[0], 0, 3000 * BASE_UNIT);
            state.increase_balance(users[1], 1, 3000 * BASE_UNIT);
            state
        };
        let orders = vec![
            Order {
                id: 0,
                account_id: users[0],
                sell_token: 0,
                buy_token: 1,
                denominator: 2000 * BASE_UNIT,
                numerator: 999 * BASE_UNIT,
                remaining_sell_amount: 2000 * BASE_UNIT,
                valid_from: 0,
                valid_until: 0,
            },
            Order {
                id: 0,
                account_id: users[1],
                sell_token: 1,
                buy_token: 0,
                denominator: 999 * BASE_UNIT,
                numerator: 1996 * BASE_UNIT,
                remaining_sell_amount: 999 * BASE_UNIT,
                valid_from: 0,
                valid_until: 0,
            },
        ];

        let fee = Some(Fee {
            token: 0,
            ratio: 0.001,
        });
        let solver = NaiveSolver::new(fee.clone());
        let res = solver
            .find_prices(&orders, &state, Duration::default(), 0)
            .now_or_never()
            .unwrap()
            .unwrap();

        assert!(res.is_non_trivial());
        check_solution(&orders, res, &fee).unwrap();
    }

    #[test]
    fn test_does_not_trade_non_fee_tokens() {
        let orders = vec![
            Order {
                id: 0,
                account_id: Address::from_low_u64_be(0),
                sell_token: 0,
                buy_token: 1,
                denominator: 20000,
                numerator: 9990,
                remaining_sell_amount: 20000,
                valid_from: 0,
                valid_until: 0,
            },
            Order {
                id: 0,
                account_id: Address::from_low_u64_be(0),
                sell_token: 1,
                buy_token: 0,
                denominator: 9990,
                numerator: 19960,
                remaining_sell_amount: 9990,
                valid_from: 0,
                valid_until: 0,
            },
        ];
        let state = AccountState::with_balance_for(&orders);

        let fee = Some(Fee {
            token: 2,
            ratio: 0.001,
        });
        let solver = NaiveSolver::new(fee);
        let res = solver
            .find_prices(&orders, &state, Duration::default(), 0)
            .now_or_never()
            .unwrap()
            .unwrap();
        assert!(!res.is_non_trivial());
    }

    #[test]
    fn test_empty_orders() {
        let orders: Vec<Order> = vec![];
        let state = AccountState::with_balance_for(&orders);

        let solver = NaiveSolver::new(None);
        let res = solver
            .find_prices(&orders, &state, Duration::default(), 0)
            .now_or_never()
            .unwrap()
            .unwrap();
        assert_eq!(res, Solution::trivial());
    }

    #[test]
    fn test_empty_sell_volume() {
        let orders = vec![
            Order {
                id: 0,
                account_id: Address::from_low_u64_be(0),
                sell_token: 0,
                buy_token: 1,
                denominator: 0,
                numerator: 0,
                remaining_sell_amount: 0,
                valid_from: 0,
                valid_until: 0,
            },
            Order {
                id: 0,
                account_id: Address::from_low_u64_be(0),
                sell_token: 1,
                buy_token: 0,
                denominator: 0,
                numerator: 0,
                remaining_sell_amount: 0,
                valid_from: 0,
                valid_until: 0,
            },
        ];
        let state = AccountState::with_balance_for(&orders);

        let solver = NaiveSolver::new(None);
        let res = solver
            .find_prices(&orders, &state, Duration::default(), 0)
            .now_or_never()
            .unwrap()
            .unwrap();
        assert!(!res.is_non_trivial());
    }

    #[test]
    fn test_match_and_unmatchable_order() {
        let fee = Some(Fee {
            token: 0,
            ratio: 0.5,
        });
        let orders = [
            Order {
                id: 0,
                account_id: Address::from_low_u64_be(0),
                sell_token: 0,
                buy_token: 1,
                denominator: 20 * BASE_UNIT,
                numerator: 10 * BASE_UNIT,
                remaining_sell_amount: 20 * BASE_UNIT,
                valid_from: 0,
                valid_until: 0,
            },
            Order {
                id: 1,
                account_id: Address::from_low_u64_be(1),
                sell_token: 1,
                buy_token: 0,
                denominator: 10 * BASE_UNIT,
                numerator: 5 * BASE_UNIT,
                remaining_sell_amount: 10 * BASE_UNIT,
                valid_from: 0,
                valid_until: 0,
            },
            Order {
                id: 2,
                account_id: Address::from_low_u64_be(2),
                sell_token: 0,
                buy_token: 2,
                denominator: BASE_UNIT,
                numerator: BASE_UNIT,
                remaining_sell_amount: BASE_UNIT,
                valid_from: 0,
                valid_until: 0,
            },
        ];
        let state = AccountState::with_balance_for(&orders);

        let solver = NaiveSolver::new(fee.clone());
        let res = solver
            .find_prices(&orders, &state, Duration::default(), 0)
            .now_or_never()
            .unwrap()
            .unwrap();
        check_solution(&orders, res, &fee).unwrap();
    }

    #[test]
    fn test_multiple_matches() {
        let fee = Some(Fee {
            token: 0,
            ratio: 0.5,
        });
        let orders = [
            Order {
                id: 0,
                account_id: Address::from_low_u64_be(0),
                sell_token: 0,
                buy_token: 1,
                denominator: 20 * BASE_UNIT,
                numerator: 10 * BASE_UNIT,
                remaining_sell_amount: 20 * BASE_UNIT,
                valid_from: 0,
                valid_until: 0,
            },
            Order {
                id: 1,
                account_id: Address::from_low_u64_be(1),
                sell_token: 1,
                buy_token: 0,
                denominator: 10 * BASE_UNIT,
                numerator: 5 * BASE_UNIT,
                remaining_sell_amount: 10 * BASE_UNIT,
                valid_from: 0,
                valid_until: 0,
            },
            Order {
                id: 2,
                account_id: Address::from_low_u64_be(2),
                sell_token: 0,
                buy_token: 2,
                denominator: 20 * BASE_UNIT,
                numerator: 10 * BASE_UNIT,
                remaining_sell_amount: 20 * BASE_UNIT,
                valid_from: 0,
                valid_until: 0,
            },
            Order {
                id: 3,
                account_id: Address::from_low_u64_be(3),
                sell_token: 2,
                buy_token: 0,
                denominator: 10 * BASE_UNIT,
                numerator: 5 * BASE_UNIT,
                remaining_sell_amount: 10 * BASE_UNIT,
                valid_from: 0,
                valid_until: 0,
            },
        ];
        let state = AccountState::with_balance_for(&orders);

        let solver = NaiveSolver::new(fee.clone());
        let res = solver
            .find_prices(&orders, &state, Duration::default(), 0)
            .now_or_never()
            .unwrap()
            .unwrap();
        check_solution(&orders, res, &fee).unwrap();
    }

    fn order_pair_first_fully_matching_second() -> Vec<Order> {
        vec![
            Order {
                id: 0,
                account_id: Address::from_low_u64_be(1),
                sell_token: 0,
                buy_token: 1,
                denominator: 52 * BASE_UNIT,
                numerator: 4 * BASE_UNIT,
                remaining_sell_amount: 52 * BASE_UNIT,
                valid_from: 0,
                valid_until: 0,
            },
            Order {
                id: 0,
                account_id: Address::from_low_u64_be(0),
                sell_token: 1,
                buy_token: 0,
                denominator: 15 * BASE_UNIT,
                numerator: 180 * BASE_UNIT,
                remaining_sell_amount: 15 * BASE_UNIT,
                valid_from: 0,
                valid_until: 0,
            },
        ]
    }

    fn order_pair_both_fully_matched() -> Vec<Order> {
        vec![
            Order {
                id: 0,
                account_id: Address::from_low_u64_be(1),
                sell_token: 2,
                buy_token: 1,
                denominator: 10 * BASE_UNIT,
                numerator: 10 * BASE_UNIT,
                remaining_sell_amount: 10 * BASE_UNIT,
                valid_from: 0,
                valid_until: 0,
            },
            Order {
                id: 1,
                account_id: Address::from_low_u64_be(1),
                sell_token: 1,
                buy_token: 2,
                denominator: 16 * BASE_UNIT,
                numerator: 8 * BASE_UNIT,
                remaining_sell_amount: 16 * BASE_UNIT,
                valid_from: 0,
                valid_until: 0,
            },
        ]
    }

    fn check_solution(
        orders: &[Order],
        solution: Solution,
        fee: &Option<Fee>,
    ) -> Result<(), String> {
        if !solution.is_non_trivial() {
            // trivial solutions are always OK
            return Ok(());
        }

        if let Some(fee_token) = fee.as_ref().map(|fee| fee.token) {
            if solution.price(fee_token).unwrap_or_default() != BASE_PRICE {
                return Err(format!(
                    "price of fee token does not match the base price: {} != {}",
                    solution.prices[&fee_token], BASE_PRICE
                ));
            }
        }

        let mut token_conservation = HashMap::new();
        for (i, order) in orders.iter().enumerate() {
            let buy_token_price = *solution.prices.get(&order.buy_token).unwrap_or(&0u128);
            let sell_token_price = *solution.prices.get(&order.sell_token).unwrap_or(&0u128);

            let exec_buy_amount = solution
                .executed_orders
                .iter()
                .find(|executed_order| {
                    executed_order.account_id == order.account_id
                        && executed_order.order_id == order.id
                })
                .map(|executed_order| executed_order.buy_amount)
                .unwrap_or(0);
            let exec_sell_amount = if sell_token_price > 0 {
                if let Some(fee) = fee {
                    let fee_denominator = (1.0 / fee.ratio) as u128;
                    // We compute:
                    // sell_amount_wo_fee = buy_amount * buy_token_price / sell_token_price
                    // sell_amount_w_fee = sell_amount_wo_fee * fee_denominator / (fee_denominator - 1)
                    // Rearranged to avoid 256 bit overflow and still have minimal rounding error.
                    ((U256::from(exec_buy_amount) * U256::from(buy_token_price))
                        / U256::from(fee_denominator - 1)
                        * U256::from(fee_denominator)
                        / U256::from(sell_token_price))
                    .as_u128()
                } else {
                    (exec_buy_amount * buy_token_price) / sell_token_price
                }
            } else {
                0
            };

            if exec_sell_amount > order.remaining_sell_amount {
                return Err(format!(
                    "ExecutedSellAmount for order {} bigger than allowed ({} > {})",
                    i, exec_sell_amount, order.remaining_sell_amount
                ));
            }

            let limit_lhs = U256::from(exec_sell_amount) * U256::from(order.numerator);
            let limit_rhs = U256::from(exec_buy_amount) * U256::from(order.denominator);
            if limit_lhs > limit_rhs {
                return Err(format!(
                    "LimitPrice for order {} not satisfied ({} > {})",
                    i, limit_lhs, limit_rhs
                ));
            }

            *token_conservation.entry(order.buy_token).or_insert(0) += exec_buy_amount as i128;
            *token_conservation.entry(order.sell_token).or_insert(0) -= exec_sell_amount as i128;
        }

        for token_id in solution.prices.keys() {
            let balance = token_conservation.entry(*token_id).or_insert(0);
            if *balance != 0 && (fee.is_none() || *token_id != fee.as_ref().unwrap().token) {
                return Err(format!(
                    "Token balance of token {} not 0 (was {})",
                    token_id, balance
                ));
            }
        }
        Ok(())
    }
}
