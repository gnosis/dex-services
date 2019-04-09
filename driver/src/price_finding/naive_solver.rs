use crate::models;

use super::price_finder_interface::Solution;
use web3::types::U256;
use crate::models::Order;
use itertools::Itertools;
use std::error::Error;

enum OrderPairType {
    TypeIa,
    TypeIb,
    TypeII
}

impl Order {
    fn matches(&self, other: &Order) -> bool {
        self.opposite_tokens(other) && self.have_price_overlap(other)
    }

    fn match_compare(&self, other: &Order) -> Option<OrderPairType> {
        if self.matches(other) {
            if self.buy_amount <= other.sell_amount && self.sell_amount <= other.buy_amount {
                Some(OrderPairType::TypeIa)
            } else if self.buy_amount >= other.sell_amount && self.sell_amount >= other.buy_amount {
                Some(OrderPairType::TypeIb)
            } else {
                Some(OrderPairType::TypeII)
            }
        }
        None()
    }

    fn opposite_tokens(&self, other: &Order) -> bool {
        self.buy_token == other.sell_token && self.sell_token == other.buy_token
    }

    fn have_price_overlap(&self, other: &Order) -> bool {
        self.buy_amount * other.buy_amount <= other.sell_amount * self.sell_amount
    }
}

pub fn solve(orders: &Vec<models::Order>, num_tokens: u8) -> Result<Solution, Error> {
    // TODO - use mapping here
    let mut prices: Vec<u128> = vec![0; num_tokens as usize];
    let mut sell_amount: Vec<u128> = vec![0; orders.len()];
    let mut buy_amount: Vec<u128> = vec![0; orders.len()];

    for (i, x) in orders.iter().enumerate() {
        for j in i + 1..orders.len() {
            let y = &orders[j];

            match x.match_compare(y) {
                Some(OrderPairType::TypeIa) => {
                    prices[x.buy_token - 1] = x.sell_amount;
                    prices[y.buy_token - 1] = x.buy_amount;

                    sell_amount[i] = x.sell_amount;
                    sell_amount[j] = x.buy_amount;

                    buy_amount[i] = x.buy_amount;
                    buy_amount[j] = x.sell_amount;
                },
                Some(OrderPairType::TypeIb) => {
                    prices[x.sell_token - 1] = y.sell_amount;
                    prices[y.sell_token - 1] = y.buy_amount;

                    sell_amount[i] = y.buy_amount;
                    sell_amount[j] = y.sell_amount;

                    buy_amount[i] = y.sell_amount;
                    buy_amount[j] = y.buy_amount;
                },
                Some(OrderPairType::TypeII) => {
                    prices[x.buy_token - 1] = y.sell_amount;
                    prices[y.buy_token - 1] = x.sell_amount;

                    sell_amount[i] = x.sell_amount;
                    sell_amount[j] = y.sell_amount;

                    buy_amount[i] = y.sell_amount;
                    buy_amount[j] = x.sell_amount;
                },
                None => None
            }
            Ok(Solution {
                surplus: U256([0, 0, 0, 0]),
                prices,
                executed_sell_amounts: sell_amount,
                executed_buy_amounts: buy_amount,
            })
        }
    }

    Ok(Solution {
        surplus: U256([0, 0, 0, 0]),
        prices,
        executed_sell_amounts: sell_amount,
        executed_buy_amounts: buy_amount,
    })
}