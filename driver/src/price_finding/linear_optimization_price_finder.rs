use crate::price_finding::error::{ErrorKind, PriceFindingError};
use crate::price_finding::price_finder_interface::{Fee, PriceFinding};

use dfusion_core::models;

use chrono::Utc;
use log::{debug, error};
use std::collections::{HashMap, HashSet};
use std::fs::{create_dir_all, File};
use std::io::{BufReader, BufWriter, Read, Write};
use std::process::Command;
use web3::types::H160;

const RESULT_FOLDER: &str = "./results/tmp/";

type PriceMap = HashMap<u16, u128>;

mod solver_output {
    use serde::Deserialize;
    use std::collections::HashMap;
    use std::vec::Vec;

    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    pub struct Order {
        pub exec_sell_amount: Option<String>,
        pub exec_buy_amount: Option<String>,
    }

    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    pub struct Output {
        pub orders: Vec<Order>,
        pub prices: HashMap<String, Option<String>>,
    }
}

mod solver_input {
    use serde::Serialize;
    use std::collections::HashMap;
    use std::vec::Vec;

    #[derive(Serialize)]
    #[serde(rename_all = "camelCase")]
    pub struct Order {
        #[serde(rename = "accountID")]
        pub account_id: String,
        pub sell_token: String,
        pub buy_token: String,
        pub sell_amount: String,
        pub buy_amount: String,
    }

    #[derive(Serialize)]
    pub struct Fee {
        pub token: String,
        pub ratio: f64,
    }

    pub type Accounts = HashMap<String, HashMap<String, String>>;

    #[derive(Serialize)]
    #[serde(rename_all = "camelCase")]
    pub struct Input {
        pub tokens: Vec<String>,
        pub ref_token: String,
        pub accounts: Accounts,
        pub orders: Vec<Order>,
        pub fee: Option<Fee>,
    }
}

pub struct LinearOptimisationPriceFinder {
    // default IO methods can be replaced for unit testing
    write_input: fn(&str, &str) -> std::io::Result<()>,
    run_solver: fn(&str) -> Result<(), PriceFindingError>,
    read_output: fn() -> std::io::Result<String>,
    fee: Option<Fee>,
}

impl LinearOptimisationPriceFinder {
    pub fn new(fee: Option<Fee>) -> Self {
        create_dir_all("instances").expect("Could not create instance directory");
        LinearOptimisationPriceFinder {
            write_input,
            run_solver,
            read_output,
            fee,
        }
    }
}

fn token_id(token: u16) -> String {
    format!("token{}", token)
}

fn account_id(account: H160) -> String {
    format!("{:x}", account)
}

fn serialize_tokens(orders: &[models::Order]) -> Vec<String> {
    // Get collection of all token ids appearing in orders
    let mut token_ids = orders.iter().map(|o| o.buy_token).collect::<HashSet<u16>>();
    token_ids.extend(orders.iter().map(|o| o.sell_token));

    token_ids.into_iter().map(token_id).collect::<Vec<String>>()
}

fn serialize_balances(
    state: &models::AccountState,
    orders: &[models::Order],
) -> solver_input::Accounts {
    let mut accounts = HashMap::new();
    for order in orders {
        let modify_token_balance = |token_balance: &mut HashMap<String, String>| {
            let sell_balance = state
                .read_balance(order.sell_token, order.account_id)
                .to_string();
            let buy_balance = state
                .read_balance(order.buy_token, order.account_id)
                .to_string();
            token_balance.insert(token_id(order.sell_token), sell_balance);
            token_balance.insert(token_id(order.buy_token), buy_balance);
        };
        accounts
            .entry(account_id(order.account_id))
            .and_modify(modify_token_balance)
            .or_insert_with(|| {
                let mut token_balance = HashMap::new();
                modify_token_balance(&mut token_balance);
                token_balance
            });
    }
    accounts
}

fn serialize_order(order: &models::Order) -> solver_input::Order {
    solver_input::Order {
        account_id: account_id(order.account_id),
        sell_token: token_id(order.sell_token),
        buy_token: token_id(order.buy_token),
        sell_amount: order.sell_amount.to_string(),
        buy_amount: order.buy_amount.to_string(),
    }
}

fn serialize_fee(fee: &Option<Fee>) -> Option<solver_input::Fee> {
    fee.as_ref().map(|fee| solver_input::Fee {
        token: token_id(fee.token),
        ratio: fee.ratio,
    })
}

fn parse_token(key: &str) -> Result<u16, PriceFindingError> {
    if key.starts_with("token") {
        return key[5..].parse::<u16>().map_err(|err| {
            PriceFindingError::new(
                format!("Failed to parse token id: {}", err).as_ref(),
                ErrorKind::ParseIntError,
            )
        });
    }
    Err(PriceFindingError::new(
        "Token keys expected to start with \"token\"",
        ErrorKind::JsonError,
    ))
}

fn parse_price(price: &Option<String>) -> Result<u128, PriceFindingError> {
    price.as_ref().map_or(Ok(0), |price| {
        price.parse().map_err(PriceFindingError::from)
    })
}

fn deserialize_result(result: String) -> Result<models::Solution, PriceFindingError> {
    let output: solver_output::Output = serde_json::from_str(&result)?;

    let prices = output
        .prices
        .iter()
        .map(|(token, price)| -> Result<_, PriceFindingError> {
            Ok((parse_token(token)?, parse_price(price)?))
        })
        .collect::<Result<PriceMap, PriceFindingError>>()?;

    let executed_sell_amounts = output
        .orders
        .iter()
        .map(|o| parse_price(&o.exec_sell_amount))
        .collect::<Result<Vec<u128>, PriceFindingError>>()?;

    let executed_buy_amounts = output
        .orders
        .iter()
        .map(|o| parse_price(&o.exec_buy_amount))
        .collect::<Result<Vec<u128>, PriceFindingError>>()?;

    Ok(models::Solution {
        prices,
        executed_sell_amounts,
        executed_buy_amounts,
    })
}

impl PriceFinding for LinearOptimisationPriceFinder {
    fn find_prices(
        &self,
        orders: &[models::Order],
        state: &models::AccountState,
    ) -> Result<models::Solution, PriceFindingError> {
        let input = solver_input::Input {
            tokens: serialize_tokens(&orders),
            ref_token: token_id(0),
            accounts: serialize_balances(&state, &orders),
            orders: orders.iter().map(serialize_order).collect(),
            fee: serialize_fee(&self.fee),
        };
        let input_file = format!("instances/instance_{}.json", Utc::now().to_rfc3339());
        (self.write_input)(&input_file, &serde_json::to_string(&input)?)?;
        (self.run_solver)(&input_file)?;
        let result = (self.read_output)()?;
        let solution = deserialize_result(result)?;
        Ok(solution)
    }
}

fn write_input(input_file: &str, input: &str) -> std::io::Result<()> {
    let file = File::create(&input_file)?;
    let mut writer = BufWriter::new(file);
    writer.write_all(input.as_bytes())?;
    debug!("Solver input: {}", input);
    Ok(())
}

fn run_solver(input_file: &str) -> Result<(), PriceFindingError> {
    let output = Command::new("python")
        .args(&["-m", "batchauctions.scripts.e2e._run"])
        .arg(RESULT_FOLDER)
        .args(&["--jsonFile", input_file])
        .output()?;

    if !output.status.success() {
        error!(
            "Solver failed - stdout: {}, error: {}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        return Err(PriceFindingError::new(
            "Solver execution failed",
            ErrorKind::ExecutionError,
        ));
    }
    Ok(())
}

fn read_output() -> std::io::Result<String> {
    let file = File::open(format!("{}{}", RESULT_FOLDER, "06_solution_int_valid.json"))?;
    let mut reader = BufReader::new(file);
    let mut result = String::new();
    reader.read_to_string(&mut result)?;
    debug!("Solver result: {}", &result);
    Ok(result)
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use dfusion_core::models::account_state::test_util::*;
    use dfusion_core::models::util::map_from_slice;
    use serde_json::json;
    use std::error::Error;
    use web3::types::{H256, U256};

    #[test]
    fn test_parse_prices() {
        // 2**128 should not fit into a u128 (max value is 2**128-1)
        let err = parse_price(&Some("340282366920938463463374607431768211457".to_owned()))
            .expect_err("Should fail");
        assert_eq!(err.kind, ErrorKind::ParseIntError);
    }

    #[test]
    fn test_serialize_order() {
        let order = models::Order {
            batch_information: None,
            account_id: H160::from_low_u64_be(0),
            sell_token: 1,
            buy_token: 2,
            sell_amount: 100,
            buy_amount: 200,
        };
        let result = serialize_order(&order);
        assert_eq!(result.sell_token, "token1");
        assert_eq!(result.buy_token, "token2");
        assert_eq!(result.sell_amount, "100");
        assert_eq!(result.buy_amount, "200");
        assert_eq!(
            result.account_id,
            "0000000000000000000000000000000000000000"
        );
    }

    #[test]
    fn test_serialize_tokens() {
        let orders = [
            models::Order {
                sell_token: 4,
                buy_token: 2,
                ..models::Order::default()
            },
            models::Order {
                sell_token: 2,
                buy_token: 0,
                ..models::Order::default()
            },
        ];
        let mut result = serialize_tokens(&orders);
        result.sort_unstable();
        let expected = vec!["token0", "token2", "token4"];
        assert_eq!(result, expected);
    }

    #[test]
    fn test_deserialize_result() {
        let json = json!({
            "prices": {
                "token0": "14024052566155238000",
                "token1": "170141183460469231731687303715884105728", // greater than max value of u64
                "token2": null,
            },
            "orders": [
                {
                    "execSellAmount": "0",
                    "execBuyAmount": "0"
                },
                {
                    "execSellAmount": "318390084925498118944",
                    "execBuyAmount": "95042777139162480000"
                },
            ]
        });

        let expected_solution = models::Solution {
            prices: map_from_slice(&[
                (0, 14_024_052_566_155_238_000),
                (1, 170_141_183_460_469_231_731_687_303_715_884_105_728),
                (2, 0),
            ]),
            executed_sell_amounts: vec![0, 318_390_084_925_498_118_944],
            executed_buy_amounts: vec![0, 95_042_777_139_162_480_000],
        };

        let solution = deserialize_result(json.to_string()).expect("Should not fail to parse");
        assert_eq!(solution, expected_solution);
    }

    #[test]
    fn test_failed_deserialize_result() {
        let json = json!({
            "The Prices": {
                "tokenA": "1",
                "tokenB": "2",
            },
        });
        let err = deserialize_result(json.to_string()).expect_err("Should fail to parse");
        assert_eq!(err.kind, ErrorKind::JsonError);

        let json = json!({
            "orders": [],
            "prices": {
                "tkn1": "1",
            },
        });
        let err = deserialize_result(json.to_string()).expect_err("Should fail to parse");
        assert_eq!(
            err.description(),
            "Token keys expected to start with \"token\""
        );

        let json = json!({
            "orders": [],
            "prices": {
                "tokenX": "1",
            },
        });
        let err = deserialize_result(json.to_string()).expect_err("Should fail to parse");
        assert_eq!(
            err.description(),
            "Failed to parse token id: invalid digit found in string"
        );

        let json = json!({
            "orders": [],
            "prices": {
                "token9999999999": "1",
            },
        });
        let err = deserialize_result(json.to_string()).expect_err("Should fail to parse");
        assert_eq!(
            err.description(),
            "Failed to parse token id: number too large to fit in target type"
        );
    }

    #[test]
    fn serialize_result_fails_if_prices_missing() {
        let json = json!({
            "orders": []
        });
        let err = deserialize_result(json.to_string()).expect_err("Should fail to parse");
        assert_eq!(err.kind, ErrorKind::JsonError);
    }

    #[test]
    fn serialize_result_fails_if_orders_missing() {
        let json = json!({
            "prices": {
                "token0": "100",
            },
        });
        let err = deserialize_result(json.to_string()).expect_err("Should fail to parse");
        assert_eq!(err.kind, ErrorKind::JsonError);
    }

    #[test]
    fn serialize_result_assumes_zero_if_order_does_not_have_sell_amount() {
        let json = json!({
            "prices": {
                "token0": "100",
            },
            "orders": [
                {
                    "execBuyAmount": "0"
                }
            ]
        });
        let result = deserialize_result(json.to_string()).expect("Should not fail to parse");
        assert_eq!(result.executed_sell_amounts[0], 0);
    }

    #[test]
    fn serialize_result_fails_if_order_sell_volume_not_parsable() {
        let json = json!({
            "prices": {
                "token0": "100",
            },
            "orders": [
                {
                    "execSellAmount": "0a",
                    "execBuyAmount": "0"
                }
            ]
        });
        let err = deserialize_result(json.to_string()).expect_err("Should fail to parse");
        assert_eq!(err.kind, ErrorKind::ParseIntError);
    }

    #[test]
    fn serialize_result_assumes_zero_if_order_does_not_have_buy_amount() {
        let json = json!({
            "prices": {
                "token0": "100",
            },
            "orders": [
                {
                    "execSellAmount": "0"
                }
            ]
        });
        let result = deserialize_result(json.to_string()).expect("Should not fail to parse");
        assert_eq!(result.executed_buy_amounts[0], 0);
    }

    #[test]
    fn serialize_result_fails_if_order_buy_volume_not_parsable() {
        let json = json!({
            "prices": {
                "token0": "100",
            },
            "orders": [
                {
                    "execSellAmount": "0",
                    "execBuyAmount": "0a"
                }
            ]
        });
        let err = deserialize_result(json.to_string()).expect_err("Should fail to parse");
        assert_eq!(err.kind, ErrorKind::ParseIntError);
    }

    #[test]
    fn test_serialize_balances() {
        let state = models::AccountState::new(
            H256::zero(),
            U256::zero(),
            vec![100, 200, 300, 400, 500, 600],
            3,
        );
        let orders = [
            models::Order {
                batch_information: None,
                account_id: H160::from_low_u64_be(0),
                sell_token: 1,
                buy_token: 2,
                sell_amount: 100,
                buy_amount: 200,
            },
            models::Order {
                batch_information: None,
                account_id: H160::from_low_u64_be(1),
                sell_token: 2,
                buy_token: 1,
                sell_amount: 200,
                buy_amount: 100,
            },
        ];
        let result = serialize_balances(&state, &orders);
        let mut expected = solver_input::Accounts::new();
        let mut first = HashMap::new();
        first.insert("token1".to_string(), "200".to_string());
        first.insert("token2".to_string(), "300".to_string());
        expected.insert(
            "0000000000000000000000000000000000000000".to_string(),
            first,
        );
        let mut second = HashMap::new();
        second.insert("token1".to_string(), "500".to_string());
        second.insert("token2".to_string(), "600".to_string());
        expected.insert(
            "0000000000000000000000000000000000000001".to_string(),
            second,
        );
        assert_eq!(result, expected)
    }

    #[test]
    fn test_serialize_input_with_fee() {
        let fee = Fee {
            token: 0,
            ratio: 0.001,
        };
        let solver = LinearOptimisationPriceFinder {
            write_input: |_, content: &str| {
                let json: serde_json::value::Value = serde_json::from_str(content).unwrap();
                assert_eq!(
                    json["fee"],
                    json!({
                        "token": "token0",
                        "ratio": 0.001
                    })
                );
                Ok(())
            },
            run_solver: |_| Ok(()),
            read_output: || Err(std::io::Error::last_os_error()),
            fee: Some(fee),
        };
        let orders = vec![];
        assert!(solver
            .find_prices(&orders, &create_account_state_with_balance_for(&orders))
            .is_err());
    }
}
