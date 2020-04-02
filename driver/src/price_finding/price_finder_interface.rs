use crate::models;
use anyhow::{anyhow, Error, Result};
#[cfg(test)]
use mockall::automock;
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
    pub fn to_args(self, result_folder: &str, input_file: &str, time_limit: String) -> Vec<String> {
        let standard_solver_command: Vec<String> = vec![
            String::from("-m"),
            String::from("batchauctions.scripts.e2e._run"),
            input_file.to_owned(),
            format!("--outputDir={}", result_folder),
            format!("--solverTimeLimit={}", time_limit),
        ];
        let fallback_solver_command: Vec<String> = vec![
            // lots of duplication. Can't do anything about it right now,
            // due tothread::spawn(move || {
            //    |         let fallback_solver_command: Vec<String> =
            //    |                                      ----------- expected due to this
            // 55 |             standard_solver_command.append(vec![String::from("--useExternalPrices")]);
            //    |             ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ expected struct `std::vec::Vec`, found `()`
            //    |
            //    = note: expected struct `std::vec::Vec<std::string::String>`
            //            found unit type `()`
            String::from("-m"),
            String::from("batchauctions.scripts.e2e._run"),
            input_file.to_owned(),
            format!("--outputDir={}", result_folder),
            format!("--solverTimeLimit={}", time_limit),
            String::from("--useExternalPrices"),
        ];
        let open_solver_command = vec![
            String::from("-m"),
            String::from("open-solver.src.match"),
            input_file.to_owned(),
            format!(
                "--solution={}{}",
                result_folder.to_owned(),
                "06_solution_int_valid.json",
            ),
            String::from("best-token-pair"),
        ];
        match self {
            SolverType::OpenSolver => open_solver_command,
            SolverType::StandardSolver => standard_solver_command,
            SolverType::FallbackSolver => fallback_solver_command,
            SolverType::NaiveSolver => {
                panic!("OptimizationSolver should not be called with naive solver")
            }
        }
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
