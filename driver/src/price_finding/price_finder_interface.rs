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
}

impl FromStr for SolverType {
    type Err = Error;

    fn from_str(solver_type_str: &str) -> Result<Self> {
        match solver_type_str.to_lowercase().as_str() {
            "standard-solver" => Ok(SolverType::StandardSolver),
            "fallback-solver" => Ok(SolverType::FallbackSolver),
            "naive-solver" => Ok(SolverType::NaiveSolver),
            _ => Err(anyhow!("solver type does not exit")),
        }
    }
}

impl SolverType {
    pub fn to_args(self) -> Vec<String> {
        match self {
            SolverType::StandardSolver => vec![],
            SolverType::FallbackSolver => vec![
                String::from("--useExternalPrices"),
            ],
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
