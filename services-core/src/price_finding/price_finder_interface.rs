use crate::models;
use anyhow::Result;
use log::debug;
use std::process::Command;
use std::process::Output;
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

arg_enum! {
    /// The internal optimizer used by the standard and fallback solvers.
    #[derive(Clone, Copy, Debug)]
    pub enum InternalOptimizer {
        Scip,
        Gurobi,
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

arg_enum! {
    #[derive(Clone, Debug, Copy, PartialEq)]
    pub enum SolverType {
        NaiveSolver,
        StandardSolver,
        OpenSolver,
        BestRingSolver,
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
                execute_open_solver(result_folder, input_file, time_limit, min_avg_fee_per_order)
            }
            SolverType::StandardSolver | SolverType::BestRingSolver => execute_private_solver(
                result_folder,
                input_file,
                time_limit,
                min_avg_fee_per_order,
                OptModel::TwoStage,
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
    time_limit: String,
    min_avg_fee_per_order: u128,
) -> Result<Output> {
    let mut command = Command::new("gp_match");
    let open_solver_command = command
        .arg(input_file)
        .arg(format!(
            "--solution={}{}",
            result_folder.to_owned(),
            "06_solution_int_valid.json",
        ))
        .arg("--logging=WARNING")
        .arg(format!("--time-limit={}", time_limit))
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
        .arg(input_file)
        .arg(format!("--outputDir={}", result_folder))
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

#[cfg_attr(test, mockall::automock)]
#[async_trait::async_trait]
pub trait PriceFinding {
    async fn find_prices(
        &self,
        orders: &[models::Order],
        state: &models::AccountState,
        time_limit: Duration,
        min_avg_earned_fee: u128,
    ) -> Result<models::Solution>;
}
