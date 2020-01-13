use crate::price_finding::error::{ErrorKind, PriceFindingError};
use crate::price_finding::price_finder_interface::{Fee, PriceFinding};

use dfusion_core::models;

use chrono::Utc;
use log::{debug, error};
use serde_json::json;
use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::io::BufReader;
use std::iter::FromIterator;
use std::process::Command;
use web3::types::H160;

const RESULT_FOLDER: &str = "./results/tmp/";

type PriceMap = HashMap<u16, u128>;

pub struct LinearOptimisationPriceFinder {
    // default IO methods can be replaced for unit testing
    write_input: fn(&str, &serde_json::Value) -> std::io::Result<()>,
    run_solver: fn(&str) -> Result<(), PriceFindingError>,
    read_output: fn() -> std::io::Result<serde_json::Value>,
    fee: Option<Fee>,
}

impl LinearOptimisationPriceFinder {
    pub fn new(fee: Option<Fee>) -> Self {
        // All prices are 1 (10**18)
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
    let mut token_ids = orders.iter().map(|o| o.buy_token).collect::<Vec<u16>>();
    token_ids.extend(orders.iter().map(|o| o.sell_token).collect::<Vec<u16>>());

    // Remove duplicate tokens by casting to HashSet and convert back to Vec
    let unique_token_ids: HashSet<u16> = HashSet::from_iter(token_ids.iter().cloned());
    let mut token_vec = unique_token_ids.into_iter().collect::<Vec<u16>>();

    // unstable sort has no performance loss since elements are unique
    token_vec.sort_unstable();

    token_vec.iter().map(|t| token_id(*t)).collect()
}

fn serialize_balances(state: &models::AccountState, orders: &[models::Order]) -> serde_json::Value {
    let mut accounts: HashMap<String, HashMap<String, String>> = HashMap::new();
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
    json!(accounts)
}

fn serialize_order(order: &models::Order, id: &str) -> serde_json::Value {
    json!({
        "accountID": account_id(order.account_id),
        "sellToken": token_id(order.sell_token),
        "buyToken": token_id(order.buy_token),
        "sellAmount": order.sell_amount.to_string(),
        "buyAmount": order.buy_amount.to_string(),
        "ID": id //TODO this should not be needed
    })
}

fn parse_token(key: &str) -> Result<u16, PriceFindingError> {
    if key.len() < 6 {
        return Err(PriceFindingError::new(
            format!(
                "Insufficient key length {} (expected at least 6)",
                key.len()
            )
            .as_ref(),
            ErrorKind::JsonError,
        ));
    }
    key[5..]
        .parse::<u16>()
        .map_err(|_| PriceFindingError::new("Failed to parse token id", ErrorKind::ParseIntError))
}

fn parse_price_value(value: &serde_json::Value) -> Result<u128, PriceFindingError> {
    value
        .as_str()
        .ok_or_else(|| PriceFindingError::from("Price value not a string"))?
        .parse::<u128>()
        .map_err(|_| {
            PriceFindingError::new("Failed to parse price string", ErrorKind::ParseIntError)
        })
}

fn parse_price(
    key: &str,
    value: &serde_json::value::Value,
) -> Result<(u16, u128), PriceFindingError> {
    Ok((parse_token(key)?, parse_price_value(value)?))
}

fn deserialize_result(json: &serde_json::Value) -> Result<models::Solution, PriceFindingError> {
    let prices = json["prices"]
        .as_object()
        .ok_or_else(|| "No 'price' object in json")?
        .iter()
        .map(|(token, price)| parse_price(token, price))
        .collect::<Result<PriceMap, PriceFindingError>>()?;

    let orders = json["orders"]
        .as_array()
        .ok_or_else(|| "No 'orders' list in json")?;
    let executed_sell_amounts = orders
        .iter()
        .map(|o| {
            o["execSellAmount"]
                .as_str()
                .unwrap_or("0")
                .parse::<u128>()
                .map_err(PriceFindingError::from)
        })
        .collect::<Result<Vec<u128>, PriceFindingError>>()?;
    let executed_buy_amounts = orders
        .iter()
        .map(|o| {
            o["execBuyAmount"]
                .as_str()
                .unwrap_or("0")
                .parse::<u128>()
                .map_err(PriceFindingError::from)
        })
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
        let token_ids = serialize_tokens(&orders);
        let accounts = serialize_balances(&state, &orders);
        let orders: Vec<serde_json::Value> = orders
            .iter()
            .enumerate()
            .map(|(index, order)| serialize_order(&order, &index.to_string()))
            .collect();
        let mut input = json!({
            "tokens": token_ids,
            "refToken": token_id(0),
            "accounts": accounts,
            "orders": orders,
            "pricesPrev": HashMap::<String, String>::new(),
        });
        if let Some(fee) = &self.fee {
            input["fee"] = json!({
                "token": token_id(fee.token),
                "ratio": fee.ratio,
            });
        }
        let input_file = format!("instance_{}.json", Utc::now().to_rfc3339());
        (self.write_input)(&input_file, &input)?;
        (self.run_solver)(&input_file)?;
        let result = (self.read_output)()?;
        let solution = deserialize_result(&result)?;
        Ok(solution)
    }
}

fn write_input(input_file: &str, input: &serde_json::Value) -> std::io::Result<()> {
    let file = File::create(&input_file)?;
    serde_json::to_writer(file, input)?;
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

fn read_output() -> std::io::Result<serde_json::Value> {
    let file = File::open(format!("{}{}", RESULT_FOLDER, "06_solution_int_valid.json"))?;
    let reader = BufReader::new(file);
    let value = serde_json::from_reader(reader)?;
    debug!("Solver result: {}", &value);
    Ok(value)
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use dfusion_core::models::account_state::test_util::*;
    use dfusion_core::models::util::map_from_slice;
    use std::error::Error;
    use web3::types::{H256, U256};

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
        let result = serialize_order(&order, "1");
        let expected = json!({
            "sellToken": "token1",
            "buyToken": "token2",
            "sellAmount": "100",
            "buyAmount": "200",
            "accountID": "0000000000000000000000000000000000000000",
            "ID": "1"
        });
        assert_eq!(result, expected);
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
        let result = serialize_tokens(&orders);
        let expected = vec!["token0", "token2", "token4"];
        assert_eq!(result, expected);
    }

    #[test]
    fn test_deserialize_result() {
        let json = json!({
            "prices": {
                "token0": "14024052566155238000",
                "token1": "1526784674855762300",
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
                (1, 1_526_784_674_855_762_300),
            ]),
            executed_sell_amounts: vec![0, 318_390_084_925_498_118_944],
            executed_buy_amounts: vec![0, 95_042_777_139_162_480_000],
        };

        let solution = deserialize_result(&json).expect("Should not fail to parse");
        assert_eq!(solution, expected_solution);
    }

    #[test]
    fn test_failed_deserialize_result() {
        let json = json!({
            "The Prices": {
                "tokenA": 1,
                "tokenB": "2",
            },
        });
        let err = deserialize_result(&json).expect_err("Should fail to parse");
        assert_eq!(err.description(), "No 'price' object in json");

        let json = json!({
            "prices": {
                "tkn1": 1,
            },
        });
        let err = deserialize_result(&json).expect_err("Should fail to parse");
        assert_eq!(
            err.description(),
            "Insufficient key length 4 (expected at least 6)"
        );

        let json = json!({
            "prices": {
                "token1": 1,
            },
        });
        let err = deserialize_result(&json).expect_err("Should fail to parse");
        assert_eq!(err.description(), "Price value not a string");

        let json = json!({
            "prices": {
                "tokenX": "1",
            },
        });
        let err = deserialize_result(&json).expect_err("Should fail to parse");
        assert_eq!(err.description(), "Failed to parse token id");

        let json = json!({
            "prices": {
                "token9999999999": "1",
            },
        });
        let err = deserialize_result(&json).expect_err("Should fail to parse");
        assert_eq!(err.description(), "Failed to parse token id");
    }

    #[test]
    fn serialize_result_fails_if_prices_missing() {
        let json = json!({
            "orders": []
        });
        let err = deserialize_result(&json).expect_err("Should fail to parse");
        assert_eq!(err.description(), "No 'price' object in json");
    }

    #[test]
    fn serialize_result_fails_if_orders_missing() {
        let json = json!({
            "prices": {
                "token0": "100",
            },
        });
        let err = deserialize_result(&json).expect_err("Should fail to parse");
        assert_eq!(err.description(), "No 'orders' list in json");
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
        let result = deserialize_result(&json).expect("Should not fail to parse");
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
        let err = deserialize_result(&json).expect_err("Should fail to parse");
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
        let result = deserialize_result(&json).expect("Should not fail to parse");
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
        let err = deserialize_result(&json).expect_err("Should fail to parse");
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
        let expected = json!({
            "0000000000000000000000000000000000000000": {
                "token1": "200",
                "token2": "300",
            },
            "0000000000000000000000000000000000000001": {
                "token1": "500",
                "token2": "600",
            }
        });
        assert_eq!(result, expected)
    }

    #[test]
    fn test_serialize_input_with_fee() {
        let fee = Fee {
            token: 0,
            ratio: 0.001,
        };
        let solver = LinearOptimisationPriceFinder {
            write_input: |_, json: &serde_json::Value| {
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
