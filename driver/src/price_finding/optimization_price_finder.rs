use crate::models::{self, TokenId, TokenInfo};
use crate::price_estimation::PriceEstimating;
use crate::price_finding::price_finder_interface::{Fee, PriceFinding, SolverType};

use anyhow::{anyhow, Context, Result};
use chrono::Utc;
use log::{debug, error};
use serde::{Deserialize, Serialize};
use serde_with::rust::display_fromstr;
use std::collections::BTreeMap;
use std::fs::{create_dir_all, File};
use std::io::{BufReader, BufWriter, Read, Write};
use std::time::Duration;

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

pub type TokenDataType = BTreeMap<TokenId, Option<TokenInfo>>;

mod solver_output {
    use super::{Num, TokenId};
    use crate::models::Solution;
    use ethcontract::Address;
    use serde::Deserialize;
    use std::collections::HashMap;

    /// Order executed buy and sell amounts.
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    pub struct ExecutedOrder {
        #[serde(rename = "accountID")]
        pub account_id: Address,
        #[serde(rename = "orderID")]
        pub order_id: u16,
        #[serde(default)]
        pub exec_sell_amount: Num,
        #[serde(default)]
        pub exec_buy_amount: Num,
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

            let executed_orders = self
                .orders
                .iter()
                .map(|order| crate::models::ExecutedOrder {
                    account_id: order.account_id,
                    order_id: order.order_id,
                    buy_amount: order.exec_buy_amount.0,
                    sell_amount: order.exec_sell_amount.0,
                })
                .collect();

            Solution {
                prices,
                executed_orders,
            }
        }
    }
}

mod solver_input {
    use super::{Num, TokenDataType, TokenId};
    use crate::models;
    use crate::price_finding;
    use ethcontract::Address;
    use serde::Serialize;
    use std::collections::BTreeMap;
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
        pub account_id: Address,
        pub sell_token: TokenId,
        pub buy_token: TokenId,
        pub sell_amount: Num,
        pub buy_amount: Num,
        #[serde(rename = "orderID")]
        pub order_id: u16,
    }

    impl From<&'_ models::Order> for Order {
        fn from(order: &models::Order) -> Self {
            Order {
                account_id: order.account_id,
                sell_token: TokenId(order.sell_token),
                buy_token: TokenId(order.buy_token),
                sell_amount: Num(order.sell_amount),
                buy_amount: Num(order.buy_amount),
                order_id: order.id,
            }
        }
    }

    pub type Accounts = BTreeMap<Address, BTreeMap<TokenId, Num>>;

    /// JSON serializable solver input data.
    #[derive(Serialize)]
    #[serde(rename_all = "camelCase")]
    pub struct Input {
        pub tokens: TokenDataType,
        pub ref_token: TokenId,
        pub accounts: Accounts,
        pub orders: Vec<Order>,
        pub fee: Option<Fee>,
    }
}

#[cfg_attr(test, mockall::automock)]
trait Io {
    fn write_input(&self, input_file: &str, input: &str) -> std::io::Result<()>;
    fn run_solver(
        &self,
        input_file: &str,
        result_folder: &str,
        solver_type: SolverType,
        time_limit: Duration,
    ) -> Result<()>;
    fn read_output(&self, result_folder: &str) -> std::io::Result<String>;
}

pub struct OptimisationPriceFinder {
    io_methods: Box<dyn Io + Sync>,
    fee: Option<Fee>,
    solver_type: SolverType,
    price_oracle: Box<dyn PriceEstimating + Sync>,
}

impl OptimisationPriceFinder {
    pub fn new(
        fee: Option<Fee>,
        solver_type: SolverType,
        price_oracle: impl PriceEstimating + Sync + 'static,
    ) -> Self {
        OptimisationPriceFinder {
            io_methods: Box::new(DefaultIo),
            fee,
            solver_type,
            price_oracle: Box::new(price_oracle),
        }
    }
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

fn deserialize_result(result: String) -> Result<models::Solution> {
    let output: solver_output::Output = serde_json::from_str(&result)?;
    Ok(output.to_solution())
}

impl PriceFinding for OptimisationPriceFinder {
    fn find_prices(
        &self,
        orders: &[models::Order],
        state: &models::AccountState,
        time_limit: Duration,
    ) -> Result<models::Solution> {
        let input = solver_input::Input {
            tokens: self.price_oracle.get_token_prices(&orders),
            ref_token: TokenId(0),
            accounts: serialize_balances(&state, &orders),
            orders: orders.iter().map(From::from).collect(),
            fee: self.fee.as_ref().map(From::from),
        };

        let now = Utc::now();
        // We are solving the batch before the current one
        let batch_id = (now.timestamp() / 300) - 1;
        let date = now.format("%Y-%m-%d");

        let input_folder = format!("instances/{}", &date);
        let input_file = format!(
            "{}/instance_{}_{}.json",
            &input_folder,
            &batch_id,
            &now.to_rfc3339()
        );

        let result_folder = format!(
            "results/{}/instance_{}_{}/",
            &date,
            &batch_id,
            &now.to_rfc3339()
        );

        create_dir_all(&input_folder)?;
        create_dir_all(&result_folder)?;

        self.io_methods
            .write_input(&input_file, &serde_json::to_string(&input)?)
            .with_context(|| format!("error writing instance to {}", input_file))?;
        self.io_methods
            .run_solver(&input_file, &result_folder, self.solver_type, time_limit)
            .with_context(|| format!("error running {:?} solver", self.solver_type))?;
        let result = self
            .io_methods
            .read_output(&result_folder)
            .with_context(|| format!("error reading solver output from {}", &result_folder))?;
        let solution = deserialize_result(result).context("error deserializing solver output")?;
        Ok(solution)
    }
}

pub struct DefaultIo;

impl Io for DefaultIo {
    fn write_input(&self, input_file: &str, input: &str) -> std::io::Result<()> {
        let file = File::create(&input_file)?;
        let mut writer = BufWriter::new(file);
        writer.write_all(input.as_bytes())?;
        Ok(())
    }

    fn run_solver(
        &self,
        input_file: &str,
        result_folder: &str,
        solver: SolverType,
        time_limit: Duration,
    ) -> Result<()> {
        let time_limit = (time_limit.as_secs_f64().round() as u64).to_string();
        let output = solver.execute(result_folder, input_file, time_limit)?;

        if !output.status.success() {
            error!(
                "Solver failed - stdout: {}, error: {}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
            return Err(anyhow!("Solver execution failed"));
        }
        Ok(())
    }

    fn read_output(&self, result_folder: &str) -> std::io::Result<String> {
        let file = File::open(format!("{}{}", result_folder, "06_solution_int_valid.json"))?;
        let mut reader = BufReader::new(file);
        let mut result = String::new();
        reader.read_to_string(&mut result)?;
        Ok(result)
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use crate::models::AccountState;
    use crate::price_estimation::MockPriceEstimating;
    use crate::util::test_util::map_from_slice;
    use ethcontract::Address;
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
    fn test_deserialize_result() {
        let json = json!({
            "prices": {
                "T0000": "14024052566155238000",
                "T0001": "170141183460469231731687303715884105728", // greater than max value of u64
                "T0002": null,
            },
            "orders": [
                {
                    "accountID": "0x0000000000000000000000000000000000000000",
                    "orderID": 0,
                    "execSellAmount": "0",
                    "execBuyAmount": "0"
                },
                {
                    "accountID": "0x0000000000000000000000000000000000000000",
                    "orderID": 1,
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

            executed_orders: vec![
                crate::models::ExecutedOrder {
                    account_id: Address::zero(),
                    order_id: 0,
                    sell_amount: 0,
                    buy_amount: 0,
                },
                crate::models::ExecutedOrder {
                    account_id: Address::zero(),
                    order_id: 1,
                    sell_amount: 318_390_084_925_498_118_944,
                    buy_amount: 95_042_777_139_162_480_000,
                },
            ],
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
                    "accountID": "0x0000000000000000000000000000000000000000",
                    "orderID": 0,
                    "execBuyAmount": "0"
                }
            ]
        });
        let result = deserialize_result(json.to_string())
            .map_err(|err| err.to_string())
            .expect("Should not fail to parse");
        assert_eq!(result.executed_orders[0].sell_amount, 0);
    }

    #[test]
    fn serialize_result_fails_if_order_sell_volume_not_parsable() {
        let json = json!({
            "prices": {
                "T0000": "100",
            },
            "orders": [
                {
                    "accountID": "0x0000000000000000000000000000000000000000",
                    "orderID": 0,
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
                    "accountID": "0x0000000000000000000000000000000000000000",
                    "orderID": 0,
                    "execSellAmount": "0"
                }
            ]
        });
        let result = deserialize_result(json.to_string()).expect("Should not fail to parse");
        assert_eq!(result.executed_orders[0].buy_amount, 0);
    }

    #[test]
    fn serialize_result_fails_if_order_buy_volume_not_parsable() {
        let json = json!({
            "prices": {
                "T0000": "100",
            },
            "orders": [
                {
                    "accountID": "0x0000000000000000000000000000000000000000",
                    "orderID": 0,
                    "execSellAmount": "0",
                    "execBuyAmount": "0a"
                }
            ]
        });
        deserialize_result(json.to_string()).expect_err("Should fail to parse");
    }

    #[test]
    fn test_serialize_balances() {
        let state = models::AccountState::new(vec![100, 200, 300, 400, 500, 600], 3);
        let orders = [
            models::Order {
                id: 0,
                account_id: Address::from_low_u64_be(0),
                sell_token: 1,
                buy_token: 2,
                sell_amount: 100,
                buy_amount: 200,
            },
            models::Order {
                id: 0,
                account_id: Address::from_low_u64_be(1),
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
        expected.insert(Address::zero(), first);

        let mut second = BTreeMap::new();
        second.insert(TokenId(1), Num(500));
        second.insert(TokenId(2), Num(600));
        expected.insert(Address::from_low_u64_be(1), second);
        assert_eq!(result, expected)
    }

    #[test]
    fn test_serialize_input_with_fee() {
        let fee = Fee {
            token: 0,
            ratio: 0.001,
        };

        let mut price_oracle = MockPriceEstimating::new();
        price_oracle
            .expect_get_token_prices()
            .withf(|orders| orders == [])
            .returning(|_| {
                btree_map! {
                    TokenId(0) => Some(TokenInfo::new("T1", 18, 1_000_000_000_000_000_000)),
                }
            });

        let mut io_methods = MockIo::new();
        io_methods
            .expect_write_input()
            .times(1)
            .withf(|_, content: &str| {
                let json: serde_json::value::Value = serde_json::from_str(content).unwrap();
                json["fee"]
                    == json!({
                        "token": "T0000",
                        "ratio": 0.001
                    })
            })
            .returning(|_, _| Ok(()));
        io_methods
            .expect_run_solver()
            .times(1)
            .returning(|_, _, _, _| Ok(()));
        io_methods
            .expect_read_output()
            .times(1)
            .returning(|_| Err(std::io::Error::last_os_error()));
        let solver = OptimisationPriceFinder {
            io_methods: Box::new(io_methods),
            fee: Some(fee),
            solver_type: SolverType::StandardSolver,
            price_oracle: Box::new(price_oracle),
        };
        let orders = vec![];
        assert!(solver
            .find_prices(
                &orders,
                &AccountState::with_balance_for(&orders),
                Duration::from_secs(180)
            )
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

        let tokens = btree_map! {
            TokenId(1) => None,
            TokenId(2) => Some(TokenInfo::new("T1", 18, 1_000_000_000_000_000_000)),
        };

        let orders = [
            models::Order {
                id: 0,
                account_id: Address::from_low_u64_be(0),
                sell_token: 1,
                buy_token: 2,
                sell_amount: 100,
                buy_amount: 200,
            },
            models::Order {
                id: 0,
                account_id: Address::from_low_u64_be(1),
                sell_token: 2,
                buy_token: 1,
                sell_amount: 200,
                buy_amount: 100,
            },
        ]
        .to_vec();
        let input = solver_input::Input {
            // tokens should also end up sorted in the end
            tokens,
            ref_token: TokenId::reference(),
            accounts,
            orders: orders.iter().map(From::from).collect(),
            fee: None,
        };
        let result = serde_json::to_string(&input).expect("Unable to serialize account state");
        assert_eq!(
            result,
            r#"{"tokens":{"T0001":null,"T0002":{"alias":"T1","decimals":18,"externalPrice":1000000000000000000}},"refToken":"T0000","accounts":{"0x13a0b42b9c180065510615972858bf41d1972a55":{},"0x4fd7c947ca0aba9d8678885e2b8c4d6a4e946984":{"T0000":"100","T0001":"100","T0002":"100","T0003":"100"}},"orders":[{"accountID":"0x0000000000000000000000000000000000000000","sellToken":"T0001","buyToken":"T0002","sellAmount":"100","buyAmount":"200","orderID":0},{"accountID":"0x0000000000000000000000000000000000000001","sellToken":"T0002","buyToken":"T0001","sellAmount":"200","buyAmount":"100","orderID":0}],"fee":null}"#
        );
    }
}
