mod data;

use self::data::{Num, TokenId};
use crate::models;
use crate::price_finding::error::{ErrorKind, PriceFindingError};
use crate::price_finding::price_finder_interface::{Fee, OptimizationModel, PriceFinding};

use chrono::Utc;
use log::{debug, error};
use std::collections::BTreeSet;
use std::fs::{create_dir_all, File};
use std::io::{BufReader, BufWriter, Read, Write};
use std::process::Command;

pub struct OptimisationPriceFinder {
    // default IO methods can be replaced for unit testing
    write_input: fn(&str, &str) -> std::io::Result<()>,
    run_solver: fn(&str, &str, OptimizationModel) -> Result<(), PriceFindingError>,
    read_output: fn(&str) -> std::io::Result<String>,
    fee: Option<Fee>,
    optimization_model: OptimizationModel,
}

impl OptimisationPriceFinder {
    pub fn new(fee: Option<Fee>, optimization_model: OptimizationModel) -> Self {
        create_dir_all("instances").expect("Could not create instance directory");
        OptimisationPriceFinder {
            write_input,
            run_solver,
            read_output,
            fee,
            optimization_model,
        }
    }
}

fn serialize_tokens(orders: &[models::Order]) -> BTreeSet<TokenId> {
    // Get collection of all token ids appearing in orders
    orders
        .iter()
        .flat_map(|o| vec![TokenId(o.buy_token), TokenId(o.sell_token)])
        .collect()
}

fn serialize_balances(state: &models::AccountState, orders: &[models::Order]) -> data::Accounts {
    let mut accounts = data::Accounts::new();
    for order in orders {
        let token_balances = accounts.entry(order.account_id).or_default();
        for &token in &[order.buy_token, order.sell_token] {
            let balance = state.read_balance(token, order.account_id);
            if balance > 0 {
                token_balances.insert(TokenId(token), Num(balance));
            }
        }
    }
    accounts
}

fn deserialize_result(result: String) -> Result<models::Solution, PriceFindingError> {
    let output: data::Output = serde_json::from_str(&result)?;
    Ok(output.to_solution())
}

impl PriceFinding for OptimisationPriceFinder {
    fn find_prices(
        &self,
        orders: &[models::Order],
        state: &models::AccountState,
    ) -> Result<models::Solution, PriceFindingError> {
        let input = data::Input {
            tokens: serialize_tokens(&orders),
            ref_token: TokenId(0),
            accounts: serialize_balances(&state, &orders),
            orders: orders.iter().map(From::from).collect(),
            fee: self.fee.as_ref().map(From::from),
        };
        let current_time = Utc::now().to_rfc3339();
        let input_file = format!("instances/instance_{}.json", &current_time);
        let result_folder = format!("results/instance_{}/", &current_time);
        (self.write_input)(&input_file, &serde_json::to_string(&input)?)?;
        (self.run_solver)(&input_file, &result_folder, self.optimization_model)?;
        let result = (self.read_output)(&result_folder)?;
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

fn run_solver(
    input_file: &str,
    result_folder: &str,
    optimization_model: OptimizationModel,
) -> Result<(), PriceFindingError> {
    let optimization_model_str = optimization_model.to_args();
    let output = Command::new("python")
        .args(&["-m", "batchauctions.scripts.e2e._run"])
        .arg(result_folder)
        .args(&["--jsonFile", input_file])
        .args(&[optimization_model_str])
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

fn read_output(result_folder: &str) -> std::io::Result<String> {
    let file = File::open(format!("{}{}", result_folder, "06_solution_int_valid.json"))?;
    let mut reader = BufReader::new(file);
    let mut result = String::new();
    reader.read_to_string(&mut result)?;
    debug!("Solver result: {}", &result);
    Ok(result)
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use crate::models::AccountState;
    use crate::util::test_util::map_from_slice;
    use ethcontract::{H160, H256, U256};
    use serde_json::json;
    use std::collections::BTreeMap;

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
        let expected = [TokenId(0), TokenId(2), TokenId(4)];
        assert!(result.iter().eq(&expected));
    }

    #[test]
    fn test_deserialize_result() {
        let json = json!({
            "prices": {
                "T0000": "14024052566155238000",
                "T0001": "170141183460469231731687303715884105728", // greater than max value of u64
                "T0002": null,
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
    fn test_serialize_balances() {
        let state = models::AccountState::new(
            H256::zero(),
            U256::zero(),
            vec![100, 200, 300, 400, 500, 600],
            3,
        );
        let orders = [
            models::Order {
                id: 0,
                account_id: H160::from_low_u64_be(0),
                sell_token: 1,
                buy_token: 2,
                sell_amount: 100,
                buy_amount: 200,
            },
            models::Order {
                id: 0,
                account_id: H160::from_low_u64_be(1),
                sell_token: 2,
                buy_token: 1,
                sell_amount: 200,
                buy_amount: 100,
            },
        ];
        let result = serialize_balances(&state, &orders);
        let mut expected = data::Accounts::new();

        let mut first = BTreeMap::new();
        first.insert(TokenId(1), Num(200));
        first.insert(TokenId(2), Num(300));
        expected.insert(H160::zero(), first);

        let mut second = BTreeMap::new();
        second.insert(TokenId(1), Num(500));
        second.insert(TokenId(2), Num(600));
        expected.insert(H160::from_low_u64_be(1), second);
        assert_eq!(result, expected)
    }

    #[test]
    fn test_serialize_input_with_fee() {
        let fee = Fee {
            token: 0,
            ratio: 0.001,
        };
        let solver = OptimisationPriceFinder {
            write_input: |_, content: &str| {
                let json: serde_json::value::Value = serde_json::from_str(content).unwrap();
                assert_eq!(
                    json["fee"],
                    json!({
                        "token": "T0000",
                        "ratio": 0.001
                    })
                );
                Ok(())
            },
            run_solver: |_, _, _| Ok(()),
            read_output: |_| Err(std::io::Error::last_os_error()),
            fee: Some(fee),
            optimization_model: OptimizationModel::MIP,
        };
        let orders = vec![];
        assert!(solver
            .find_prices(&orders, &AccountState::with_balance_for(&orders))
            .is_err());
    }
}
