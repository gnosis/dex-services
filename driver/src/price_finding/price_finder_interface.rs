use crate::models;
use anyhow::{anyhow, Error, Result};
#[cfg(test)]
use mockall::automock;

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
pub enum SolverConfig {
    NaiveSolver,
    StandardSolver { solver_time_limit: u32 },
    FallbackSolver { solver_time_limit: u32 },
}

impl SolverConfig {
    pub fn new(solver_type_str: &str, solver_time_limit: u32) -> Result<Self> {
        match solver_type_str.to_lowercase().as_str() {
            "standard-solver" => Ok(SolverConfig::StandardSolver { solver_time_limit }),
            "fallback-solver" => Ok(SolverConfig::FallbackSolver { solver_time_limit }),
            "naive-solver" => Ok(SolverConfig::NaiveSolver),
            _ => Err(anyhow!("solver type does not exit")),
        }
    }
}

impl SolverConfig {
    pub fn to_args(self) -> Vec<String> {
        match self {
            SolverConfig::StandardSolver { solver_time_limit } => {
                vec![format!("--solverTimeLimit={:}", solver_time_limit)]
            }
            SolverConfig::FallbackSolver { solver_time_limit } => vec![
                format!("--solverTimeLimit={:}", solver_time_limit),
                String::from("--useExternalPrices"),
            ],
            SolverConfig::NaiveSolver => {
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
