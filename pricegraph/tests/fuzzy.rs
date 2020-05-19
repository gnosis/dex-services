//! Module containing unexpected panics discovered by fuzzy testing.

use pricegraph::*;
use primitive_types::{H160, U256};

#[test]
fn nan_failure() {
    let elements = vec![
        Element {
            user: H160::zero(),
            balance: U256::from_dec_str(
                "12715861048125203197891841262599926257840766446203714759525741717161676636160",
            )
            .unwrap(),
            pair: TokenPair { buy: 0, sell: 1 },
            valid: Validity { from: 0, to: 0 },
            price: Price {
                numerator: 37_218_383_890_677_207_382_744_681_053_558_142_492,
                denominator: 43_657_760_336_300_720_852_568_604,
            },
            remaining_sell_amount: 37_363_768_194_016_619_614_905_451_585_067_810_816,
            id: 0,
        },
        Element {
            user: H160::zero(),
            balance: U256::from_dec_str(
                "12664765281704264833469460898684845727998501522769485522719856179532372122652",
            )
            .unwrap(),
            pair: TokenPair { buy: 0, sell: 1 },
            valid: Validity { from: 0, to: 0 },
            price: Price {
                numerator: 0,
                denominator: 0,
            },
            remaining_sell_amount: 13_438_235_513_639_339_688_651_946_631_354_122_240,
            id: 1,
        },
    ];

    let _ = Orderbook::from_elements(elements);
}
