use crate::models;
use std::convert::From;

#[cfg(test)]
use mockall::automock;

use super::error::PriceFindingError;

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

#[derive(Clone, Copy, PartialEq)]
pub enum SolverType {
    NaiveSolver,
    StandardSolver,
    FallbackSolver,
}

impl From<&str> for SolverType {
    fn from(optimization_model_str: &str) -> SolverType {
        match optimization_model_str.to_lowercase().as_str() {
            "standard-solver" => SolverType::StandardSolver,
            "fallback-solver" => SolverType::FallbackSolver,
            // the naive solver is the standard solver.
            _ => SolverType::NaiveSolver,
        }
    }
}

impl SolverType {
    pub fn to_args(self) -> &'static str {
        match self {
            SolverType::StandardSolver => &"--optModel=mip",
            // TODO: Allow to hand over several args for the optimizer

            // The fallback solver is also running --optModel=mip, as this is the default value
            // although it is not handed over in the next line
            SolverType::FallbackSolver => {
                &"--tokenInfo=/app/batchauctions/scripts/e2e/token_info_mainnet.json"
            }
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
    ) -> Result<models::Solution, PriceFindingError>;
}
