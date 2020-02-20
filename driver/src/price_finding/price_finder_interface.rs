use crate::models;
use std::convert::Infallible;
use std::str::FromStr;

#[cfg(test)]
use mockall::automock;
use anyhow::Error;

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
}

impl FromStr for SolverType {
    type Err = Infallible;

    fn from_str(solver_type_str: &str) -> Result<Self, Self::Err> {
        match solver_type_str.to_lowercase().as_str() {
            "standard-solver" => Ok(SolverType::StandardSolver),
            "fallback-solver" => Ok(SolverType::FallbackSolver),
            // the naive solver is the standard solver.
            _ => Ok(SolverType::NaiveSolver),
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
    ) -> Result<models::Solution, Error>;
}
