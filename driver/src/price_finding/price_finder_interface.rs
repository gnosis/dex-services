use crate::models;
use std::convert::Infallible;
use std::str::FromStr;

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

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum OptimizationModel {
    NAIVE,
    MIP,
    NLP,
}

impl FromStr for OptimizationModel {
    type Err = Infallible;

    fn from_str(optimization_model_str: &str) -> Result<Self, Self::Err> {
        match optimization_model_str.to_lowercase().as_str() {
            "mip" => Ok(OptimizationModel::MIP),
            "nlp" => Ok(OptimizationModel::NLP),
            // the naive solver is the standard solver.
            _ => Ok(OptimizationModel::NAIVE),
        }
    }
}

impl OptimizationModel {
    pub fn to_args(self) -> &'static str {
        match self {
            OptimizationModel::MIP => &"--optModel=mip",
            OptimizationModel::NLP => &"--optModel=nlp",
            OptimizationModel::NAIVE => {
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
