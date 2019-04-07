use crate::models;

use super::error::PriceFindingError;
use super::price_finder_interface::Solution;
use web3::types::U256;
use crate::models::Order;


//#[derive(Clone, Debug)]
//pub struct Solution {
//    pub surplus: U256,
//    pub prices: Vec<u128>,
//    pub executed_sell_amounts: Vec<u128>,
//    pub executed_buy_amounts: Vec<u128>,
//}


impl Order {
    fn matches(&self, other: &Order) -> bool {
        self.opposite_tokens(other) && self.have_price_overlap(other)
    }

    fn opposite_tokens(&self, other: &Order) -> bool {
        self.buy_token == other.sell_token && self.sell_token == other.buy_token
    }

    fn have_price_overlap(&self, other: &Order) -> bool {
        self.buy_amount * other.buy_amount <= other.sell_amount * self.sell_amount
    }
}

//pub struct Solution {
//    pub surplus: U256,
//    pub prices: Vec<u128>,
//    pub executed_sell_amounts: Vec<u128>,
//    pub executed_buy_amounts: Vec<u128>,
//}

pub fn solve(orders: &Vec<models::Order>, num_tokens: u8) -> Result<Solution, PriceFindingError> {
    // TODO - use mapping here
    let mut prices: Vec<u128> = vec![0; num_tokens as usize];
    let mut sell_amount: Vec<u128> = vec![0; orders.len()];
    let mut buy_amount: Vec<u128> = vec![0; orders.len()];

    for (i, x) in orders.iter().enumerate() {
        for j in i + 1..orders.len() {
            let y = &orders[j];
            if x.matches(y) {

                if x.buy_amount <= y.sell_amount && x.sell_amount <= y.buy_amount {
                    // Type I-A (x <= y)
                    prices[x.buy_token - 1] = x.sell_amount;
                    prices[y.buy_token - 1] = x.buy_amount;

                    sell_amount[i] = x.sell_amount;
                    sell_amount[j] = x.buy_amount;

                    buy_amount[i] = x.buy_amount;
                    buy_amount[j] = x.sell_amount;

                } else if x.buy_amount >= y.sell_amount && x.sell_amount >= y.buy_amount {
                    // Type I-B (y <= x)
                    prices[x.buy_token - 1] = y.sell_amount;
                    prices[y.buy_token - 1] = y.buy_amount;

                    sell_amount[i] = y.sell_amount;
                    sell_amount[j] = y.buy_amount;

                    buy_amount[i] = y.buy_amount;
                    buy_amount[j] = y.sell_amount;

                } else {
                    // Type II
                    prices[x.buy_token - 1] = y.sell_amount;
                    prices[y.buy_token - 1] = x.sell_amount;

                    sell_amount[i] = x.sell_amount;
                    sell_amount[j] = y.sell_amount;

                    buy_amount[i] = y.sell_amount;
                    buy_amount[j] = x.sell_amount;

                }
                break;
            }
        }
    }

    Ok(Solution {
        surplus: U256([0, 0, 0, 0]),
        prices,
        executed_sell_amounts: sell_amount,
        executed_buy_amounts: buy_amount,
    })
}