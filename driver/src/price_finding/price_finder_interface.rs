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
    ) -> Result<Output> {
        let mut command = Command::new("python");
        let command_standard_solver = command
            .current_dir("/app/batchauctions")
            .args(&["-m", "scripts.e2e._run"])
            .arg(format!("{}{}", "/app/", input_file.to_owned()))
            .arg(format!("--outputDir={}{}", "/app/", result_folder))
            .args(&["--solverTimeLimit", &time_limit]);
        let mut command = Command::new("python");
        let command_open_solver = command
            .current_dir("/app/open_solver")
            .args(&["-m", "src.match"])
            .arg(format!("{}{}", "/app/", input_file.to_owned()))
            .arg(format!(
                "--solution={}{}{}",
                "/app/",
                result_folder.to_owned(),
                "06_solution_int_valid.json",
            ))
            .arg(String::from("best-token-pair"));
        let solver_command = match self {
            SolverType::OpenSolver => command_open_solver,
            SolverType::StandardSolver => command_standard_solver,
            SolverType::FallbackSolver => {
                command_standard_solver.arg(String::from("--useExternalPrices"));
                command_standard_solver
            }
            SolverType::NaiveSolver => {
                panic!("fn to_args should not be called by the naive solver")
            }
        };
        debug!("Using solver command `{:?}`", solver_command);
        Ok(solver_command.output()?)
    }
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
