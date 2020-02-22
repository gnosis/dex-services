use crate::models;
use anyhow::Error;
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
    StandardSolver(u32),
    FallbackSolver(u32),
}

impl SolverConfig {
    pub fn new(solver_type_str: &str, solver_time_limit: u32) -> Self {
        match solver_type_str.to_lowercase().as_str() {
            "standard-solver" => SolverConfig::StandardSolver(solver_time_limit),
            "fallback-solver" => SolverConfig::FallbackSolver(solver_time_limit),
            // the naive solver is the standard solver.
            _ => SolverConfig::NaiveSolver,
        }
    }
}

impl SolverConfig {
    pub fn to_args(self) -> String {
        match self {
            SolverConfig::StandardSolver(i) => format!("--optModel=mip --solverTimeLimit={:?}", i),
            SolverConfig::FallbackSolver(i) => {
                format!("--optModel=mip --solverTimeLimit={:?} --tokenInfo=/app/batchauctions/scripts/e2e/token_info_mainnet.json --useExternalPrices", i)
            }
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
