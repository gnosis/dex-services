use crate::models;
use anyhow::{anyhow, Error, Result};
use log::debug;
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

/// The internal optimizer used by the standard and fallback solvers.
#[derive(Clone, Copy, Debug)]
pub enum InternalOptimizer {
    Scip,
    Gurobi,
}

impl FromStr for InternalOptimizer {
    type Err = Error;
    fn from_str(string: &str) -> Result<Self> {
        match string {
            "scip" => Ok(Self::Scip),
            "gurobi" => Ok(Self::Gurobi),
            _ => Err(anyhow!("internal optimizer does not exit")),
        }
    }
}

impl InternalOptimizer {
    fn to_argument(self) -> &'static str {
        match self {
            InternalOptimizer::Scip => "SCIP",
            InternalOptimizer::Gurobi => "GUROBI",
        }
    }
}

#[derive(Clone, Debug, Copy, PartialEq)]
pub enum SolverType {
    NaiveSolver,
    StandardSolver,
    FallbackSolver,
    OpenSolver,
}

impl FromStr for SolverType {
    type Err = Error;

    fn from_str(solver_type_str: &str) -> Result<Self> {
        match solver_type_str.to_lowercase().as_str() {
            "standard-solver" => Ok(SolverType::StandardSolver),
            "fallback-solver" => Ok(SolverType::FallbackSolver),
            "naive-solver" => Ok(SolverType::NaiveSolver),
            "open-solver" => Ok(SolverType::OpenSolver),
            _ => Err(anyhow!("solver type does not exit")),
        }
    }
}

impl SolverType {
    pub fn execute(
        self,
        result_folder: &str,
        input_file: &str,
        time_limit: String,
        min_avg_fee_per_order: u128,
        internal_optimizer: InternalOptimizer,
    ) -> Result<Output> {
        match self {
            SolverType::OpenSolver => {
                execute_open_solver(result_folder, input_file, min_avg_fee_per_order)
            }
            SolverType::StandardSolver | SolverType::FallbackSolver => execute_private_solver(
                result_folder,
                input_file,
                time_limit,
                min_avg_fee_per_order,
                internal_optimizer,
            ),
            SolverType::NaiveSolver => {
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
        .arg(format!("--min-avg-fee-per-order={}", min_avg_fee_per_order))
        .arg(String::from("best-token-pair"));
    debug!("Using open-solver command `{:?}`", open_solver_command);
    Ok(open_solver_command.output()?)
}

pub fn execute_private_solver(
    result_folder: &str,
    input_file: &str,
    time_limit: String,
    min_avg_fee_per_order: u128,
    internal_optimizer: InternalOptimizer,
) -> Result<Output> {
    let mut command = Command::new("python");
    let private_solver_command = command
        .current_dir("/app/batchauctions")
        .args(&["-m", "src._run"])
        .arg(format!("{}{}", "/app/", input_file.to_owned()))
        .arg(format!("--outputDir={}{}", "/app/", result_folder))
        .arg("--logging=WARNING")
        .args(&["--solverTimeLimit", &time_limit])
        .arg(format!("--minAvgFeePerOrder={}", min_avg_fee_per_order))
        .arg(format!("--solver={}", internal_optimizer.to_argument()))
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
