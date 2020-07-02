use crate::models;
use anyhow::{anyhow, Error, Result};
use futures::future::BoxFuture;
use log::debug;
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

/// The optimization algorithm used by standard and best-ring solvers.
#[derive(Clone, Copy, Debug)]
pub enum OptModel {
    MixedInteger,
    TwoStage,
}

impl OptModel {
    fn to_argument(self) -> &'static str {
        match self {
            OptModel::MixedInteger => "mip",
            OptModel::TwoStage => "two_stage",
        }
    }
}

#[derive(Clone, Debug, Copy, PartialEq)]
pub enum SolverType {
    NaiveSolver,
    StandardSolver,
    OpenSolver,
    BestRingSolver,
}

impl FromStr for SolverType {
    type Err = Error;

    fn from_str(solver_type_str: &str) -> Result<Self> {
        match solver_type_str.to_lowercase().as_str() {
            "standard-solver" => Ok(SolverType::StandardSolver),
            "naive-solver" => Ok(SolverType::NaiveSolver),
            "open-solver" => Ok(SolverType::OpenSolver),
            "best-ring-solver" => Ok(SolverType::BestRingSolver),
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
            SolverType::StandardSolver | SolverType::BestRingSolver => execute_private_solver(
                result_folder,
                input_file,
                time_limit,
                min_avg_fee_per_order,
                if self == SolverType::StandardSolver {
                    OptModel::TwoStage
                } else {
                    OptModel::MixedInteger
                },
                internal_optimizer,
                self == SolverType::BestRingSolver,
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
        .arg("--logging=WARNING")
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
    opt_model: OptModel,
    internal_optimizer: InternalOptimizer,
    search_only_for_best_ring_solution: bool,
) -> Result<Output> {
    let mut command = Command::new("python");
    let mut private_solver_command = command
        .current_dir("/app/batchauctions")
        .args(&["-m", "src._run"])
        .arg(format!("{}{}", "/app/", input_file.to_owned()))
        .arg(format!("--outputDir={}{}", "/app/", result_folder))
        .arg("--logging=WARNING")
        .arg(format!("--timeLimit={}", time_limit))
        .arg(format!("--minAvgFeePerOrder={}", min_avg_fee_per_order))
        .arg(format!("--optModel={}", opt_model.to_argument()))
        .arg(format!("--solver={}", internal_optimizer.to_argument()))
        .arg(String::from("--useExternalPrices"));
    if search_only_for_best_ring_solution {
        private_solver_command = private_solver_command.arg(String::from("--solveBestCycle"));
    }
    debug!(
        "Using private-solver command `{:?}`",
        private_solver_command
    );
    Ok(private_solver_command.output()?)
}

pub trait PriceFinding {
    fn find_prices<'a>(
        &'a self,
        orders: &'a [models::Order],
        state: &'a models::AccountState,
        time_limit: Duration,
    ) -> BoxFuture<'a, Result<models::Solution, Error>>;
}

// We would like to tag `PriceFinding` with `mockall::automock` but mockall does not support the
// lifetime bounds on `tokens`: https://github.com/asomers/mockall/issues/134 . As a workaround
// we create a similar trait with simpler lifetimes on which mockall works.
#[cfg(test)]
mod mock {
    use super::*;
    #[mockall::automock]
    pub trait PriceFinding_ {
        fn find_prices<'a>(
            &'a self,
            orders: &[models::Order],
            state: &models::AccountState,
            time_limit: Duration,
        ) -> BoxFuture<'a, Result<models::Solution, Error>>;
    }

    impl PriceFinding for MockPriceFinding_ {
        fn find_prices<'a>(
            &'a self,
            orders: &'a [models::Order],
            state: &'a models::AccountState,
            time_limit: Duration,
        ) -> BoxFuture<'a, Result<models::Solution, Error>> {
            PriceFinding_::find_prices(self, orders, state, time_limit)
        }
    }
}
#[cfg(test)]
pub use mock::MockPriceFinding_ as MockPriceFinding;
