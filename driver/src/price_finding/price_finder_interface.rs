use crate::models;
use anyhow::{Context, Error, Result};
use log::debug;
use serde::Deserialize;

#[cfg(test)]
use mockall::automock;
use std::process::Command;
use std::process::Output;
use std::str::FromStr;
use std::time::Duration;

#[derive(Clone)]
pub struct Fee {
    pub token: u16,
    /// Value between [0, 1] mapping from 0% -> 100%
    pub ratio: f64,
}

impl Default for Fee {
    fn default() -> Self {
        Fee {
            token: 0,
            ratio: 0.001,
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
pub enum SolverConfig {
    NaiveSolver,
    StandardSolver { min_avg_fee_per_order: u128 },
    FallbackSolver { min_avg_fee_per_order: u128 },
    OpenSolver { min_avg_fee_per_order: u128 },
}

impl FromStr for SolverConfig {
    type Err = Error;

    fn from_str(solver_config: &str) -> Result<Self> {
        Ok(serde_json::from_str(solver_config)
            .context("failed to parse solver_config from JSON string")?)
    }
}

impl Default for SolverConfig {
    fn default() -> Self {
        SolverConfig::NaiveSolver
    }
}

impl SolverConfig {
    pub fn execute(
        self,
        result_folder: &str,
        input_file: &str,
        time_limit: String,
    ) -> Result<Output> {
        match self {
            SolverConfig::OpenSolver {
                min_avg_fee_per_order,
            } => execute_open_solver(result_folder, input_file, min_avg_fee_per_order),
            SolverConfig::StandardSolver {
                min_avg_fee_per_order,
            } => {
                execute_private_solver(result_folder, input_file, time_limit, min_avg_fee_per_order)
            }
            SolverConfig::FallbackSolver {
                min_avg_fee_per_order,
            } => {
                execute_private_solver(result_folder, input_file, time_limit, min_avg_fee_per_order)
            }
            SolverConfig::NaiveSolver => {
                panic!("fn execute should not be called by the naive solver")
            }
        }
    }
}

pub fn execute_open_solver(
    result_folder: &str,
    input_file: &str,
    min_avg_fee_per_order: u128,
) -> Result<Output> {
    let mut command = Command::new("python");
    let open_solver_command = command
        .current_dir("/app/open_solver")
        .args(&["-m", "src.match"])
        .arg(format!("{}{}", "/app/", input_file.to_owned()))
        .arg(format!(
            "--solution={}{}{}",
            "/app/",
            result_folder.to_owned(),
            "06_solution_int_valid.json",
        ))
        .arg(format!("--minAvgFeePerOrder={}", min_avg_fee_per_order))
        .arg(String::from("best-token-pair"));
    debug!("Using open-solver command `{:?}`", open_solver_command);
    Ok(open_solver_command.output()?)
}

pub fn execute_private_solver(
    result_folder: &str,
    input_file: &str,
    time_limit: String,
    min_avg_fee_per_order: u128,
) -> Result<Output> {
    let mut command = Command::new("python");
    let private_solver_command = command
        .current_dir("/app/batchauctions")
        .args(&["-m", "scripts.e2e._run"])
        .arg(format!("{}{}", "/app/", input_file.to_owned()))
        .arg(format!("--outputDir={}{}", "/app/", result_folder))
        .args(&["--solverTimeLimit", &time_limit])
        .arg(format!("--minAvgFeePerOrder={}", min_avg_fee_per_order))
        .arg(String::from("--useExternalPrices"));
    debug!(
        "Using private-solver command `{:?}`",
        private_solver_command
    );
    Ok(private_solver_command.output()?)
}

#[cfg_attr(test, automock)]
pub trait PriceFinding {
    fn find_prices(
        &self,
        orders: &[models::Order],
        state: &models::AccountState,
        time_limit: Duration,
    ) -> Result<models::Solution, Error>;
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn read_open_solver_config() {
        let json = r#"{
            "StandardSolver": { "min_avg_fee_per_order": 180 }
        }"#;
        let solver_config = SolverConfig::StandardSolver {
            min_avg_fee_per_order: 180,
        };
        assert_eq!(
            solver_config,
            serde_json::from_str(json).expect("Failed to parse")
        );
    }

    #[test]
    fn read_naive_solver_config() {
        let json = r#"{
            "NaiveSolver": null
        }"#;
        let solver_config = SolverConfig::NaiveSolver;
        assert_eq!(
            solver_config,
            serde_json::from_str(json).expect("Failed to parse")
        );
    }
}
