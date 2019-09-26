use crate::price_finding::error::{ErrorKind, PriceFindingError};
use crate::price_finding::price_finder_interface::{Fee, PriceFinding};

use dfusion_core::models;

use chrono::Utc;
use serde_json::json;
use std::collections::HashMap;
use std::fs::File;
use std::io::BufReader;
use std::process::Command;
use web3::types::{H160, U256};

const RESULT_FOLDER: &str = "./results/tmp/";
type Prices = HashMap<String, String>;

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

fn deserialize_result(
    json: &serde_json::Value,
    num_tokens: u16,
) -> Result<models::Solution, PriceFindingError> {
    let price_map = json["prices"]
        .as_object()
        .ok_or_else(|| "No 'price' object in json")?
        .iter()
        .map(|(token, price)| {
            price
                .as_str()
                .map(|p| (token.to_owned(), p.to_owned()))
                .unwrap_or_else(|| (token.to_owned(), "0".to_string()))
        })
        .collect::<Prices>();
    let prices = (0..num_tokens)
        .map(|t| {
            price_map
                .get(&token_id(t))
                .ok_or_else(|| {
                    PriceFindingError::new(
                        &format!("Token {} not found in price map", t),
                        ErrorKind::JsonError,
                    )
                })
                .and_then(|price| price.parse::<u128>().map_err(PriceFindingError::from))
        })
        .collect::<Result<Vec<u128>, PriceFindingError>>()?;
    let orders = json["orders"]
        .as_array()
        .ok_or_else(|| "No 'orders' list in json")?;
    let surplus = Some(
        orders
            .iter()
            .map(|o| {
                o["execUtility"]
                    .as_str()
                    .ok_or_else(|| {
                        PriceFindingError::new(
                            "No 'execUtility' field on order",
                            ErrorKind::JsonError,
                        )
                    })
                    .and_then(|surplus| {
                        U256::from_dec_str(surplus).map_err(|e| {
                            PriceFindingError::new(&format!("{:?}", e), ErrorKind::ParseIntError)
                        })
                    })
            })
            .collect::<Result<Vec<U256>, PriceFindingError>>()?
            .iter()
            .fold(U256::zero(), |acc, surplus| surplus.saturating_add(acc)),
    );
    let executed_sell_amounts = orders
        .iter()
        .map(|o| {
            o["execSellAmount"]
                .as_str()
                .ok_or_else(|| {
                    PriceFindingError::new(
                        "No 'execSellAmount' field on order",
                        ErrorKind::JsonError,
                    )
                })
                .and_then(|amount| amount.parse::<u128>().map_err(PriceFindingError::from))
        })
        .collect::<Result<Vec<u128>, PriceFindingError>>()?;
    let executed_buy_amounts = orders
        .iter()
        .map(|o| {
            o["execBuyAmount"]
                .as_str()
                .ok_or_else(|| {
                    PriceFindingError::new(
                        "No 'execBuyAmount' field on order",
                        ErrorKind::JsonError,
                    )
                })
                .and_then(|amount| amount.parse::<u128>().map_err(PriceFindingError::from))
        })
        .collect::<Result<Vec<u128>, PriceFindingError>>()?;
    Ok(models::Solution {
        surplus,
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
        let token_ids: Vec<String> = (0..models::TOKENS).map(token_id).collect();
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
        let solution = deserialize_result(&result, models::TOKENS)?;
        Ok(solution)
    }
}

fn write_input(input_file: &str, input: &serde_json::Value) -> std::io::Result<()> {
    let file = File::create(&input_file)?;
    serde_json::to_writer(file, input)?;
    Ok(())
}

fn run_solver(input_file: &str) -> Result<(), PriceFindingError> {
    let output = Command::new("python")
        .arg("./batchauctions/scripts/e2e/_run.py")
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
    Ok(value)
}

#[cfg(test)]
pub mod tests {

    use super::*;
    use dfusion_core::models::account_state::test_util::*;
    use std::error::Error;
    use web3::types::H256;

    #[test]
    fn test_serialize_order() {
        let order = models::Order {
            batch_information: None,
            account_id: H160::from(0),
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
    fn test_deserialize_result() {
        let json = json!({
            "prices": {
                "token0": "14024052566155238000",
                "token1": "1526784674855762300",
            },
            "orders": [
                {
                    "execSellAmount": "0",
                    "execBuyAmount": "0",
                    "execUtility": "0"
                },
                {
                    "execSellAmount": "318390084925498118944",
                    "execBuyAmount": "95042777139162480000",
                    "execUtility": "15854632034944469292777429010439194350"
                },
            ]
        });

        let expected_solution = models::Solution {
            surplus: U256::from_dec_str("15854632034944469292777429010439194350").ok(),
            prices: vec![14_024_052_566_155_238_000, 1_526_784_674_855_762_300],
            executed_sell_amounts: vec![0, 318_390_084_925_498_118_944],
            executed_buy_amounts: vec![0, 95_042_777_139_162_480_000],
        };

        let solution = deserialize_result(&json, 2).expect("Should not fail to parse");
        assert_eq!(solution, expected_solution);
    }

    #[test]
    fn serialize_result_fails_if_prices_missing() {
        let json = json!({
            "orders": []
        });
        let err = deserialize_result(&json, 2).expect_err("Should fail to parse");
        assert_eq!(err.description(), "No 'price' object in json");
    }

    #[test]
    fn serialize_result_fails_if_single_price_missing() {
        let json = json!({
            "prices": {
                "token0": "100",
            },
            "orders": []
        });
        let err = deserialize_result(&json, 2).expect_err("Should fail to parse");
        assert_eq!(err.description(), "Token 1 not found in price map");
    }

    #[test]
    fn serialize_result_fails_if_orders_missing() {
        let json = json!({
            "prices": {
                "token0": "100",
            },
        });
        let err = deserialize_result(&json, 1).expect_err("Should fail to parse");
        assert_eq!(err.description(), "No 'orders' list in json");
    }
    #[test]
    fn serialize_result_fails_if_order_does_not_have_surplus() {
        let json = json!({
            "prices": {
                "token0": "100",
            },
            "orders": [
                {
                    "execSellAmount": "0",
                    "execBuyAmount": "0",
                }
            ]
        });
        let err = deserialize_result(&json, 1).expect_err("Should fail to parse");
        assert_eq!(err.description(), "No 'execUtility' field on order");
    }

    #[test]
    fn serialize_result_fails_if_order_suprlus_not_parseable() {
        let json = json!({
            "prices": {
                "token0": "100",
            },
            "orders": [
                {
                    "execSellAmount": "0",
                    "execBuyAmount": "0",
                    "execUtility": "0a0b"
                }
            ]
        });
        let err = deserialize_result(&json, 1).expect_err("Should fail to parse");
        assert_eq!(err.kind, ErrorKind::ParseIntError);
    }

    #[test]
    fn serialize_result_fails_if_order_does_not_have_sell_amount() {
        let json = json!({
            "prices": {
                "token0": "100",
            },
            "orders": [
                {
                    "execBuyAmount": "0",
                    "execUtility": "0"
                }
            ]
        });
        let err = deserialize_result(&json, 1).expect_err("Should fail to parse");
        assert_eq!(err.description(), "No 'execSellAmount' field on order");
    }

    #[test]
    fn serialize_result_fails_if_order_sell_volume_not_parseable() {
        let json = json!({
            "prices": {
                "token0": "100",
            },
            "orders": [
                {
                    "execSellAmount": "0a",
                    "execBuyAmount": "0",
                    "execUtility": "0"
                }
            ]
        });
        let err = deserialize_result(&json, 1).expect_err("Should fail to parse");
        assert_eq!(err.kind, ErrorKind::ParseIntError);
    }

    #[test]
    fn serialize_result_fails_if_order_does_not_have_buy_amount() {
        let json = json!({
            "prices": {
                "token0": "100",
            },
            "orders": [
                {
                    "execSellAmount": "0",
                    "execUtility": "0"
                }
            ]
        });
        let err = deserialize_result(&json, 1).expect_err("Should fail to parse");
        assert_eq!(err.description(), "No 'execBuyAmount' field on order");
    }

    #[test]
    fn serialize_result_fails_if_order_buy_volume_not_parseable() {
        let json = json!({
            "prices": {
                "token0": "100",
            },
            "orders": [
                {
                    "execSellAmount": "0",
                    "execBuyAmount": "0a",
                    "execUtility": "0"
                }
            ]
        });
        let err = deserialize_result(&json, 1).expect_err("Should fail to parse");
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
                account_id: H160::from(0),
                sell_token: 1,
                buy_token: 2,
                sell_amount: 100,
                buy_amount: 200,
            },
            models::Order {
                batch_information: None,
                account_id: H160::from(1),
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
