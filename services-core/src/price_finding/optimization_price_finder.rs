use crate::{
    metrics::{
        solver_metrics::{SolverMetrics, SolverStats},
        StableXMetrics,
    },
    models::{self, solution::Solution, TokenId, TokenInfo},
    price_estimation::PriceEstimating,
    price_finding::price_finder_interface::{Fee, InternalOptimizer, PriceFinding, SolverType},
};
use anyhow::{anyhow, Context, Result};
use chrono::Utc;
use ethcontract::U256;
use log::error;
use serde::{Deserialize, Serialize};
use serde_with::rust::display_fromstr;
use std::collections::BTreeMap;
use std::env;
use std::fmt::{Debug, Display};
use std::fs::{create_dir_all, File};
use std::io::{BufReader, BufWriter, Read, Write};
use std::path::Path;
use std::str::FromStr;
use std::sync::Arc;
use std::time::Duration;

/// A number wrapper type that correctly serializes large integers to strings to
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
pub struct Num<T>(#[serde(with = "display_fromstr")] pub T)
where
    T: Display + FromStr,
    <T as FromStr>::Err: Display;

pub type TokenDataType = BTreeMap<TokenId, Option<TokenInfo>>;

mod solver_output {
    use super::{Num, TokenId};
    use crate::{metrics::solver_metrics::SolverStats, models::solution::Solution};
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
        pub exec_sell_amount: Num<u128>,
        #[serde(default)]
        pub exec_buy_amount: Num<u128>,
    }

    /// Solver solution output format. This format can be converted directly to
    /// the exchange solution format.
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    pub struct Output {
        pub orders: Vec<ExecutedOrder>,
        pub prices: HashMap<TokenId, Option<Num<u128>>>,
        #[serde(flatten)]
        pub solver_stats: SolverStats,
    }

    impl Output {
        /// Convert the solver output to a solution.
        pub fn into_solution(self) -> (Solution, SolverStats) {
            let prices = self
                .prices
                .into_iter()
                .filter_map(|(token, price)| Some((token.0, price?.0)))
                .filter(|(_, price)| *price > 0)
                .collect();

            let executed_orders = self
                .orders
                .into_iter()
                .map(|order| crate::models::ExecutedOrder {
                    account_id: order.account_id,
                    order_id: order.order_id,
                    buy_amount: order.exec_buy_amount.0,
                    sell_amount: order.exec_sell_amount.0,
                })
                .collect();

            (
                Solution {
                    prices,
                    executed_orders,
                },
                self.solver_stats,
            )
        }
    }
}

mod solver_input {
    use super::{Num, TokenDataType, TokenId};
    use crate::models;
    use crate::price_finding;
    use ethcontract::{Address, U256};
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
        pub sell_amount: Num<u128>,
        pub buy_amount: Num<u128>,
        #[serde(rename = "orderID")]
        pub order_id: u16,
    }

    impl From<&'_ models::Order> for Order {
        fn from(order: &models::Order) -> Self {
            // The solver does not handle remaining amounts.
            let (buy_amount, sell_amount) = order.compute_remaining_buy_sell_amounts();
            Order {
                account_id: order.account_id,
                sell_token: TokenId(order.sell_token),
                buy_token: TokenId(order.buy_token),
                sell_amount: Num(sell_amount),
                buy_amount: Num(buy_amount),
                order_id: order.id,
            }
        }
    }

    pub type Accounts = BTreeMap<Address, BTreeMap<TokenId, Num<U256>>>;

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
    #[allow(clippy::too_many_arguments)]
    fn run_solver(
        &self,
        input_file: &str,
        input: &str,
        result_folder: &str,
        solver_type: SolverType,
        time_limit: Duration,
        min_avg_fee_per_order: u128,
        internal_optimizer: InternalOptimizer,
    ) -> Result<String>;
}

pub struct OptimisationPriceFinder {
    io_methods: Arc<dyn Io + Send + Sync>,
    fee: Option<Fee>,
    solver_type: SolverType,
    price_oracle: Arc<dyn PriceEstimating + Send + Sync>,
    internal_optimizer: InternalOptimizer,
    solver_metrics: SolverMetrics,
    stablex_metrics: Arc<StableXMetrics>,
}

impl OptimisationPriceFinder {
    pub fn new(
        fee: Option<Fee>,
        solver_type: SolverType,
        price_oracle: Arc<dyn PriceEstimating + Send + Sync>,
        internal_optimizer: InternalOptimizer,
        solver_metrics: SolverMetrics,
        stablex_metrics: Arc<StableXMetrics>,
    ) -> Self {
        OptimisationPriceFinder {
            io_methods: Arc::new(DefaultIo),
            fee,
            solver_type,
            price_oracle,
            internal_optimizer,
            solver_metrics,
            stablex_metrics,
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
            if balance > U256::zero() {
                token_balances.insert(TokenId(token), Num(balance));
            }
        }
    }
    accounts
}

fn deserialize_result(result: String) -> Result<(Solution, SolverStats)> {
    let output: solver_output::Output = serde_json::from_str(&result)?;
    Ok(output.into_solution())
}

#[async_trait::async_trait]
impl PriceFinding for OptimisationPriceFinder {
    async fn find_prices(
        &self,
        orders: &[models::Order],
        state: &models::AccountState,
        time_limit: Duration,
        min_avg_earned_fee: u128,
    ) -> Result<Solution> {
        let price_oracle = &*self.price_oracle;
        let input = solver_input::Input {
            tokens: price_oracle.get_token_prices(&orders).await,
            ref_token: TokenId(0),
            accounts: serialize_balances(&state, &orders),
            orders: orders.iter().map(From::from).collect(),
            fee: self.fee.as_ref().map(From::from),
        };

        let now = Utc::now();
        // We are solving the batch before the current one
        let batch_id = (now.timestamp() / 300) - 1;
        let date = now.format("%Y-%m-%d");
        let current_directory = env::current_dir()?;

        let input_folder = format!("{}/instances/{}", current_directory.display(), &date);
        let input_file = format!(
            "{}/instance_{}_{}.json",
            &input_folder,
            &batch_id,
            &now.to_rfc3339()
        );

        let result_folder = format!(
            "{}/results/{}/instance_{}_{}/",
            &current_directory.display(),
            &date,
            &batch_id,
            &now.to_rfc3339()
        );

        // `blocking::unblock` requires the closure to be 'static.
        let io_methods = self.io_methods.clone();
        let solver_type = self.solver_type;
        // The solver expects the fee amount as the total paid fees. Half of the paid fees are
        // burned and half earned.
        let min_avg_fee = 2 * min_avg_earned_fee;
        self.stablex_metrics.min_avg_fee_calculated(min_avg_fee);
        let internal_optimizer = self.internal_optimizer;
        let result = blocking::unblock(move || {
            io_methods.run_solver(
                &input_file,
                &serde_json::to_string(&input)?,
                &result_folder,
                solver_type,
                time_limit,
                min_avg_fee,
                internal_optimizer,
            )
        })
        .await
        .with_context(|| format!("error running {:?} solver", self.solver_type))?;
        let (solution, solver_stats) =
            deserialize_result(result).context("error deserializing solver output")?;
        self.solver_metrics.handle_stats(&solver_stats);
        Ok(solution)
    }
}

pub struct DefaultIo;

impl DefaultIo {
    fn write_input(&self, input_file: &str, input: &str) -> std::io::Result<()> {
        if let Some(parent) = Path::new(input_file).parent() {
            create_dir_all(parent)?;
        }
        let file = File::create(&input_file)?;
        let mut writer = BufWriter::new(file);
        writer.write_all(input.as_bytes())?;
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

impl Io for DefaultIo {
    fn run_solver(
        &self,
        input_file: &str,
        input: &str,
        result_folder: &str,
        solver: SolverType,
        time_limit: Duration,
        min_avg_fee_per_order: u128,
        internal_optimizer: InternalOptimizer,
    ) -> Result<String> {
        self.write_input(input_file, input)
            .with_context(|| format!("error writing instance to {}", input_file))?;

        create_dir_all(result_folder)?;
        let time_limit = (time_limit.as_secs_f64().round() as u64).to_string();
        let output = solver.execute(
            result_folder,
            input_file,
            time_limit,
            min_avg_fee_per_order,
            internal_optimizer,
        )?;

        if !output.status.success() {
            error!(
                "Solver failed - stdout: {}, error: {}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
            return Err(anyhow!("Solver execution failed"));
        }

        self.read_output(result_folder)
            .with_context(|| format!("error reading solver output from {}", result_folder))
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use crate::{
        models::AccountState, price_estimation::MockPriceEstimating,
        util::test_util::map_from_slice, util::FutureWaitExt as _,
    };
    use ethcontract::Address;
    use prometheus::Registry;
    use serde_json::json;
    use std::{collections::BTreeMap, sync::Arc};

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
                "T0002": null, // null prices get removed from output
                "T0003": "0", // 0 prices get removed from output
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

        let solution = deserialize_result(json.to_string())
            .expect("Should not fail to parse")
            .0;
        assert_eq!(solution, expected_solution);
    }

    #[test]
    fn deserialize_solver_stats() {
        let json = json!({
            "prices": {},
            "orders": [],
            "objVals": {
                "a": "1",
                "b": 2
            },
            "solver": {
                "c": 2.5,
                "d": null
            }
        });
        let stats = deserialize_result(json.to_string()).unwrap().1;
        assert_eq!(stats.obj_vals.len(), 2);
        assert_eq!(stats.solver.len(), 2);
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
            .expect("Should not fail to parse")
            .0;
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
        let result = deserialize_result(json.to_string())
            .expect("Should not fail to parse")
            .0;
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
                numerator: 100,
                denominator: 200,
                remaining_sell_amount: 200,
                valid_from: 0,
                valid_until: 0,
            },
            models::Order {
                id: 0,
                account_id: Address::from_low_u64_be(1),
                sell_token: 2,
                buy_token: 1,
                denominator: 200,
                numerator: 100,
                remaining_sell_amount: 200,
                valid_from: 0,
                valid_until: 0,
            },
        ];
        let result = serialize_balances(&state, &orders);
        let mut expected = solver_input::Accounts::new();

        let mut first = BTreeMap::new();
        first.insert(TokenId(1), Num(U256::from(200)));
        first.insert(TokenId(2), Num(U256::from(300)));
        expected.insert(Address::zero(), first);

        let mut second = BTreeMap::new();
        second.insert(TokenId(1), Num(U256::from(500)));
        second.insert(TokenId(2), Num(U256::from(600)));
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
            .expect_run_solver()
            .times(1)
            .withf(move |_, content: &str, _, _, _, _, _| {
                let json: serde_json::value::Value = serde_json::from_str(content).unwrap();
                json["fee"]
                    == json!({
                        "token": "T0000",
                        "ratio": 0.001
                    })
            })
            .returning(|_, _, _, _, _, _, _| Err(anyhow!("")));
        let solver = OptimisationPriceFinder {
            io_methods: Arc::new(io_methods),
            fee: Some(fee),
            solver_type: SolverType::StandardSolver,
            price_oracle: Arc::new(price_oracle),
            internal_optimizer: InternalOptimizer::Scip,
            solver_metrics: SolverMetrics::new(Arc::new(Registry::new())),
            stablex_metrics: Arc::new(StableXMetrics::new(Arc::new(Registry::new()))),
        };
        let orders = vec![];
        assert!(solver
            .find_prices(
                &orders,
                &AccountState::with_balance_for(&orders),
                Duration::from_secs(180),
                10u128.pow(18)
            )
            .wait()
            .is_err());
    }

    #[test]
    fn test_balance_serialization() {
        let mut accounts = BTreeMap::new();

        // Balances should end up ordered by token ID
        let mut user1_balances = BTreeMap::new();
        user1_balances.insert(TokenId(3), Num(U256::from(100)));
        user1_balances.insert(TokenId(2), Num(U256::from(100)));
        user1_balances.insert(TokenId(1), Num(U256::from(100)));
        user1_balances.insert(TokenId(0), Num(U256::from(100)));

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
                denominator: 100,
                numerator: 200,
                remaining_sell_amount: 100,
                valid_from: 0,
                valid_until: 0,
            },
            models::Order {
                id: 0,
                account_id: Address::from_low_u64_be(1),
                sell_token: 2,
                buy_token: 1,
                denominator: 200,
                numerator: 100,
                remaining_sell_amount: 200,
                valid_from: 0,
                valid_until: 0,
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
