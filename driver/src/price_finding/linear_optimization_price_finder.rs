use crate::price_finding::error::{PriceFindingError, ErrorKind};
use crate::price_finding::price_finder_interface::{PriceFinding, Solution};

use dfusion_core::models;

use serde_json::json;
use web3::types::U256;
use std::collections::HashMap;
use chrono::{Utc};
use std::fs::File;
use std::process::Command;
use std::io::BufReader;

const RESULT_FOLDER: &str = "./results/tmp/";
type Prices = HashMap<String, String>;

pub struct LinearOptimisationPriceFinder {
    previous_prices: Prices,
    // default IO methods can be replaced for unit testing
    write_input: fn(&str, &serde_json::Value) -> std::io::Result<()>,
    run_solver: fn(&str) -> Result<(), PriceFindingError>,
    read_output: fn() -> std::io::Result<serde_json::Value>,
}

impl Default for LinearOptimisationPriceFinder {
    fn default() -> Self {
        Self::new()
    }
}

impl LinearOptimisationPriceFinder {
    pub fn new() -> Self {
        // All prices are 1 (10**18)
        LinearOptimisationPriceFinder {
            previous_prices: (0..models::TOKENS)
                .map(|t| (token_id(t), "1000000000000000000".to_string()))
                .collect(),
            write_input,
            run_solver,
            read_output,
        }
    }
}

fn token_id(token: u8) -> String {
    format!("token{}", token)
}

fn account_id(account: u16) -> String {
    format!("account{}", account)
}

fn serialize_balances(state: &models::AccountState,) -> serde_json::Value {
    let mut accounts: HashMap<String, HashMap<String, String>> = HashMap::new();
    for account in 0..state.accounts() {
        accounts.insert(account_id(account), (0..state.num_tokens)
            .map(|token| (token_id(token), state.read_balance(token, account).to_string()))
            .collect());
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

fn deserialize_result(json: &serde_json::Value, num_tokens: u8) -> Result<(Prices, Solution), PriceFindingError> {
    let price_map = json["prices"]
        .as_object()
        .ok_or_else(|| "No 'price' object in json")?
        .iter()
        .map(|(token, price)| price
            .as_str()
            .map(|p| (token.to_owned(), p.to_owned()))
            .ok_or_else(|| PriceFindingError::new(
                &"Could not convert price to string".to_string(), ErrorKind::JsonError))
        )
        .collect::<Result<Prices, PriceFindingError>>()?;
    let prices = (0..num_tokens)
        .map(|t| price_map.get(&token_id(t))
            .ok_or_else(|| PriceFindingError::new(&format!("Token {} not found in price map", t), ErrorKind::JsonError))
            .and_then(|price| price.parse::<u128>().map_err(PriceFindingError::from))
        )
        .collect::<Result<Vec<u128>, PriceFindingError>>()?;
    let orders = json["orders"].as_array().ok_or_else(|| "No 'orders' list in json")?;
    let surplus = orders
        .iter()
        .map(|o| o["execSurplus"]
            .as_str()
            .ok_or_else(|| PriceFindingError::new("No 'execSurplus' field on order",  ErrorKind::JsonError))
            .and_then(|surplus| U256::from_dec_str(surplus).map_err(
                |e| PriceFindingError::new(&format!("{:?}", e), ErrorKind::ParseIntError)
            ))
        )
        .collect::<Result<Vec<U256>, PriceFindingError>>()?
        .iter()
        .fold(U256::zero(), |acc, surplus| surplus.saturating_add(acc));
    let executed_sell_amounts = orders
        .iter()
        .map(|o| o["execSellAmount"]
            .as_str()
            .ok_or_else(|| PriceFindingError::new("No 'execSellAmount' field on order", ErrorKind::JsonError))
            .and_then(|amount| amount.parse::<u128>().map_err(PriceFindingError::from))
        )
        .collect::<Result<Vec<u128>, PriceFindingError>>()?;
    let executed_buy_amounts = orders
        .iter()
        .map(|o| o["execBuyAmount"]
            .as_str()
            .ok_or_else(|| PriceFindingError::new("No 'execBuyAmount' field on order",  ErrorKind::JsonError))
            .and_then(|amount| amount.parse::<u128>().map_err(PriceFindingError::from))
        )
        .collect::<Result<Vec<u128>, PriceFindingError>>()?;
    Ok((price_map.to_owned(), Solution {
        surplus,
        prices,
        executed_sell_amounts,
        executed_buy_amounts,
    }))
}

impl PriceFinding for LinearOptimisationPriceFinder {
    fn find_prices(
        &mut self, 
        orders: &[models::Order],
        state: &models::AccountState
    ) -> Result<Solution, PriceFindingError> {
        let token_ids: Vec<String> = (0..state.num_tokens)
            .map(token_id)
            .collect();
        let orders: Vec<serde_json::Value> = orders
            .iter()
            .enumerate()
            .map(|(index, order)| serialize_order(&order, &index.to_string()))
            .collect();
        let input = json!({
            "tokens": token_ids,
            "refToken": token_id(0),
            "pricesPrev": self.previous_prices,
            "accounts": serialize_balances(&state),
            "orders": orders, 
        });
        let input_file = format!("instance_{}.json", Utc::now().to_rfc3339());
        (self.write_input)(&input_file, &input)?;
        (self.run_solver)(&input_file)?;
        let result = (self.read_output)()?;
        let (prices, solution) = deserialize_result(&result, state.num_tokens)?;
        self.previous_prices = prices;
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
        .arg("../batchauctions/scripts/optimize_e2e.py")
        .arg(input_file)
        .args(&["--solverTimelimit", "120"])
        .args(&["--outputDir", RESULT_FOLDER])
        .output()?;

    if !output.status.success() {
        error!("Solver failed - stdout: {}, error: {}", String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr));
        return Err(PriceFindingError::new("Solver execution failed", ErrorKind::ExecutionError))
    }
    Ok(())
}

fn read_output() -> std::io::Result<serde_json::Value> {
    let file = File::open(format!("{}{}", RESULT_FOLDER, "03_output_snark.json"))?;
    let reader = BufReader::new(file);
    let value = serde_json::from_reader(reader)?;
    Ok(value)
}

#[cfg(test)]
pub mod tests {

    use super::*;
    use std::error::Error;
    use web3::types::H256;

    #[test]
    fn test_solver_keeps_prices_from_previous_result() {
        let state = models::AccountState::new(
            H256::zero(),
            U256::zero(),
            vec![0; 2],
            2
        );
        let return_result = || { Ok(json!({
            "prices": {
                "token0": "14024052566155238000",
                "token1": "1526784674855762300",
            },
            "orders": []
        }))};
        let mut solver = LinearOptimisationPriceFinder {
            previous_prices: HashMap::new(),
            write_input: |_, _| Ok(()),
            run_solver: |_| Ok(()),
            read_output: return_result,
        };

        solver.find_prices(&vec![], &state).expect("Should not fail");

        let expected_prices: Prices = [
            ("token0".to_owned(), "14024052566155238000".to_owned()),
            ("token1".to_owned(), "1526784674855762300".to_owned())
        ].iter().cloned().collect();

        assert_eq!(solver.previous_prices, expected_prices);
    }

    #[test]
    fn test_serialize_order() {
        let order = models::Order {
            batch_information: None,
            account_id: 0,
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
            "accountID": "account0",
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
                    "execSurplus": "0"
                },
                {
                    "execSellAmount": "318390084925498118944",
                    "execBuyAmount": "95042777139162480000",
                    "execSurplus": "15854632034944469292777429010439194350"
                },
            ]
        });
        let expected_prices: Prices = [
            ("token0".to_owned(), "14024052566155238000".to_owned()),
            ("token1".to_owned(), "1526784674855762300".to_owned())
        ].iter().cloned().collect();

        let expected_solution = Solution {
            surplus: U256::from_dec_str("15854632034944469292777429010439194350").unwrap(),
            prices: vec![14024052566155238000, 1526784674855762300],
            executed_sell_amounts: vec![0, 318390084925498118944],
            executed_buy_amounts: vec![0, 95042777139162480000],
        };

        let (prices, solution) = deserialize_result(&json, 2).expect("Should not fail to parse");
        assert_eq!(prices, expected_prices);
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
    fn serialize_result_fails_if_price_not_string() {
        let json = json!({
            "prices": {
                "token0": 100,
                "token1": 200,
            },
            "orders": []
        });
        let err = deserialize_result(&json, 2).expect_err("Should fail to parse");
        assert_eq!(err.description(), "Could not convert price to string");
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
        assert_eq!(err.description(), "No 'execSurplus' field on order");
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
                    "execSurplus": "0a0b"
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
                    "execSurplus": "0"
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
                    "execSurplus": "0"
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
                    "execSurplus": "0"
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
                    "execSurplus": "0"
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
            3
        );
        let result = serialize_balances(&state);
        let expected = json!({
            "account0": {
                "token0": "100",
                "token1": "200",
                "token2": "300",
            },
            "account1": {
                "token0": "400",
                "token1": "500",
                "token2": "600",
            }
        });
        assert_eq!(result, expected)
    }

    #[test]
    #[should_panic]
    fn test_serialize_balances_with_bad_balance_length() {
        let state = models::AccountState::new(
            H256::zero(),
            U256::zero(),
            vec![100, 200],
            30
        );
        serialize_balances(&state);
    }
}
