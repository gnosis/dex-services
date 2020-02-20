use crate::models;
use crate::price_finding::error::{ErrorKind, PriceFindingError};
use crate::price_finding::price_finder_interface::{Fee, PriceFinding, SolverType};

use chrono::Utc;
use log::{debug, error};
use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_with::rust::display_fromstr;
use std::borrow::Cow;
use std::collections::BTreeSet;
use std::fs::{create_dir_all, File};
use std::io::{BufReader, BufWriter, Read, Write};
use std::process::Command;

/// A token ID wrapper type that implements JSON serialization in the solver
/// format.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialOrd, PartialEq)]
pub struct TokenId(pub u16);

impl<'de> Deserialize<'de> for TokenId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let key = Cow::<str>::deserialize(deserializer)?;
        if !key.starts_with('T') || key.len() != 5 {
            return Err(D::Error::custom("Token ID must be of the form 'Txxxx'"));
        }

        let id = key[1..].parse::<u16>().map_err(D::Error::custom)?;
        Ok(TokenId(id))
    }
}

impl Serialize for TokenId {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        format!("T{:04}", self.0).serialize(serializer)
    }
}

/// A number wrapper type that correctly serializes large u128`s to strings to
/// avoid precision loss.
///
/// The JSON standard specifies that all numbers are `f64`s and converting to
/// `u128` -> `f64` -> `u128` is lossy. Using a string representation gets
/// around that issue.
///
/// This type should be used together with standard library generic types where
/// the serialization cannot be controlled.
#[derive(Copy, Clone, Debug, Default, Eq, PartialEq, Deserialize, Serialize)]
#[serde(transparent)]
pub struct Num(#[serde(with = "display_fromstr")] pub u128);

mod solver_output {
    use super::{Num, TokenId};
    use crate::models::Solution;
    use serde::Deserialize;
    use serde_with::rust::display_fromstr;
    use std::collections::HashMap;

    /// Order executed buy and sell amounts.
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    pub struct ExecutedOrder {
        #[serde(default, with = "display_fromstr")]
        pub exec_sell_amount: u128,
        #[serde(default, with = "display_fromstr")]
        pub exec_buy_amount: u128,
    }

    /// Solver solution output format. This format can be converted directly to
    /// the exchange solution format.
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    pub struct Output {
        pub orders: Vec<ExecutedOrder>,
        pub prices: HashMap<TokenId, Option<Num>>,
    }

    impl Output {
        /// Convert the solver output to a solution.
        pub fn to_solution(&self) -> Solution {
            let prices = self
                .prices
                .iter()
                .map(|(token, price)| (token.0, price.unwrap_or_default().0))
                .collect();
            let executed_sell_amounts = self
                .orders
                .iter()
                .map(|order| order.exec_sell_amount)
                .collect();
            let executed_buy_amounts = self
                .orders
                .iter()
                .map(|order| order.exec_buy_amount)
                .collect();

            Solution {
                prices,
                executed_sell_amounts,
                executed_buy_amounts,
            }
        }
    }
}

mod solver_input {
    use super::{Num, TokenId};
    use crate::models;
    use crate::price_finding;
    use ethcontract::H160;
    use serde::Serialize;
    use serde_with::rust::display_fromstr;
    use std::collections::{BTreeMap, BTreeSet};
    use std::vec::Vec;

    /// Fee information using `TokenId` so the JSON serialization format matches
    /// what is expected by the solver.
    ///
    /// This type may be removed if the `crate::price_finding::Fee` is converted
    /// to use `TokenId` in the future.
    #[derive(Serialize)]
    #[serde(rename_all = "camelCase")]
    pub struct Fee {
        pub token: TokenId,
        pub ratio: f64,
    }

    impl From<&'_ price_finding::Fee> for Fee {
        fn from(fee: &price_finding::Fee) -> Self {
            Fee {
                token: TokenId(fee.token),
                ratio: fee.ratio,
            }
        }
    }

    /// Order information using `TokenId` so the JSON serialization format
    /// matches what is expected by the solver.
    ///
    /// This type may be removed if the `crate::modes::Order` is converted to
    /// use `TokenId` in the future.
    #[derive(Serialize)]
    #[serde(rename_all = "camelCase")]
    pub struct Order {
        #[serde(rename = "accountID")]
        pub account_id: H160,
        pub sell_token: TokenId,
        pub buy_token: TokenId,
        #[serde(with = "display_fromstr")]
        pub sell_amount: u128,
        #[serde(with = "display_fromstr")]
        pub buy_amount: u128,
    }

    impl From<&'_ models::Order> for Order {
        fn from(order: &models::Order) -> Self {
            Order {
                account_id: order.account_id,
                sell_token: TokenId(order.sell_token),
                buy_token: TokenId(order.buy_token),
                sell_amount: order.sell_amount,
                buy_amount: order.buy_amount,
            }
        }
    }

    pub type Accounts = BTreeMap<H160, BTreeMap<TokenId, Num>>;

    /// JSON serializable solver input data.
    #[derive(Serialize)]
    #[serde(rename_all = "camelCase")]
    pub struct Input {
        pub tokens: BTreeSet<TokenId>,
        pub ref_token: TokenId,
        pub accounts: Accounts,
        pub orders: Vec<Order>,
        pub fee: Option<Fee>,
    }
}

pub struct OptimisationPriceFinder {
    // default IO methods can be replaced for unit testing
    write_input: fn(&str, &str) -> std::io::Result<()>,
    run_solver: fn(&str, &str, SolverType) -> Result<(), PriceFindingError>,
    read_output: fn(&str) -> std::io::Result<String>,
    fee: Option<Fee>,
    solver_type: SolverType,
}

impl OptimisationPriceFinder {
    pub fn new(fee: Option<Fee>, solver_type: SolverType) -> Self {
        create_dir_all("instances").expect("Could not create instance directory");
        OptimisationPriceFinder {
            write_input,
            run_solver,
            read_output,
            fee,
            solver_type,
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

fn serialize_balances(
    state: &models::AccountState,
    orders: &[models::Order],
) -> solver_input::Accounts {
    let mut accounts = solver_input::Accounts::new();
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
    let output: solver_output::Output = serde_json::from_str(&result)?;
    Ok(output.to_solution())
}

impl PriceFinding for OptimisationPriceFinder {
    fn find_prices(
        &self,
        orders: &[models::Order],
        state: &models::AccountState,
    ) -> Result<models::Solution, PriceFindingError> {
        let input = solver_input::Input {
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
        (self.run_solver)(&input_file, &result_folder, self.solver_type)?;
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
    solver_type: SolverType,
) -> Result<(), PriceFindingError> {
    let solver_type_str = solver_type.to_args();
    let output = Command::new("python")
        .args(&["-m", "batchauctions.scripts.e2e._run"])
        .arg(result_folder)
        .args(&["--jsonFile", input_file])
        .args(&[solver_type_str])
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
    fn token_id_serialization() {
        for (key, expected) in &[
            (json!("T0000"), Some(TokenId(0))),
            (json!("T0042"), Some(TokenId(42))),
            (json!("T1000"), Some(TokenId(1000))),
            (json!("T001"), None),
            (json!("T00001"), None),
            (json!("00001"), None),
            (json!("Tasdf"), None),
        ] {
            let result = TokenId::deserialize(key);
            if let Some(expected) = expected {
                assert_eq!(result.unwrap(), *expected);
                assert_eq!(json!(expected), *key);
            } else {
                assert!(result.is_err());
            }
        }
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
    fn test_failed_deserialize_result() {
        let json = json!({
            "The Prices": {
                "TA": "1",
                "TB": "2",
            },
        });
        deserialize_result(json.to_string()).expect_err("Should fail to parse");

        let json = json!({
            "orders": [],
            "prices": {
                "tkn1": "1",
            },
        });
        deserialize_result(json.to_string()).expect_err("Should fail to parse");

        let json = json!({
            "orders": [],
            "prices": {
                "TX": "1",
            },
        });
        deserialize_result(json.to_string()).expect_err("Should fail to parse");

        let json = json!({
            "orders": [],
            "prices": {
                "T9999999999": "1",
            },
        });
        deserialize_result(json.to_string()).expect_err("Should fail to parse");
    }

    #[test]
    fn serialize_result_fails_if_prices_missing() {
        let json = json!({
            "orders": []
        });
        deserialize_result(json.to_string()).expect_err("Should fail to parse");
    }

    #[test]
    fn serialize_result_fails_if_orders_missing() {
        let json = json!({
            "prices": {
                "T0000": "100",
            },
        });
        deserialize_result(json.to_string()).expect_err("Should fail to parse");
    }

    #[test]
    fn serialize_result_assumes_zero_if_order_does_not_have_sell_amount() {
        let json = json!({
            "prices": {
                "T0000": "100",
            },
            "orders": [
                {
                    "execBuyAmount": "0"
                }
            ]
        });
        let result = deserialize_result(json.to_string())
            .map_err(|err| err.to_string())
            .expect("Should not fail to parse");
        assert_eq!(result.executed_sell_amounts[0], 0);
    }

    #[test]
    fn serialize_result_fails_if_order_sell_volume_not_parsable() {
        let json = json!({
            "prices": {
                "T0000": "100",
            },
            "orders": [
                {
                    "execSellAmount": "0a",
                    "execBuyAmount": "0"
                }
            ]
        });
        deserialize_result(json.to_string()).expect_err("Should fail to parse");
    }

    #[test]
    fn serialize_result_assumes_zero_if_order_does_not_have_buy_amount() {
        let json = json!({
            "prices": {
                "T0000": "100",
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
                "T0000": "100",
            },
            "orders": [
                {
                    "execSellAmount": "0",
                    "execBuyAmount": "0a"
                }
            ]
        });
        deserialize_result(json.to_string()).expect_err("Should fail to parse");
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
        let mut expected = solver_input::Accounts::new();

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
            solver_type: SolverType::StandardSolver,
        };
        let orders = vec![];
        assert!(solver
            .find_prices(&orders, &AccountState::with_balance_for(&orders))
            .is_err());
    }

    #[test]
    fn test_balance_serialization() {
        let mut accounts = BTreeMap::new();

        // Balances should end up ordered by token ID
        let mut user1_balances = BTreeMap::new();
        user1_balances.insert(TokenId(3), Num(100));
        user1_balances.insert(TokenId(2), Num(100));
        user1_balances.insert(TokenId(1), Num(100));
        user1_balances.insert(TokenId(0), Num(100));

        // Accounts should end up sorted by account ID
        accounts.insert(
            "4fd7c947ca0aba9d8678885e2b8c4d6a4e946984".parse().unwrap(),
            user1_balances,
        );
        accounts.insert(
            "13a0b42b9c180065510615972858bf41d1972a55".parse().unwrap(),
            BTreeMap::new(),
        );

        let input = solver_input::Input {
            // tokens should also end up sorted in the end
            tokens: [TokenId(3), TokenId(2), TokenId(1), TokenId(0)]
                .iter()
                .copied()
                .collect(),
            ref_token: TokenId(0),
            accounts,
            orders: vec![],
            fee: None,
        };
        let result = serde_json::to_string(&input).expect("Unable to serialize account state");
        assert_eq!(
            result,
            r#"{"tokens":["T0000","T0001","T0002","T0003"],"refToken":"T0000","accounts":{"0x13a0b42b9c180065510615972858bf41d1972a55":{},"0x4fd7c947ca0aba9d8678885e2b8c4d6a4e946984":{"T0000":"100","T0001":"100","T0002":"100","T0003":"100"}},"orders":[],"fee":null}"#
        );
    }
}
