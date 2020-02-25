#![allow(clippy::ptr_arg)] // required for automock

use crate::contracts;
use crate::contracts::stablex_contract::StableXContract;
use crate::models::{Order, Solution};

use anyhow::{Error, Result};
use ethcontract::errors::{ExecutionError, MethodError};
use ethcontract::web3::futures::Future as F;
use ethcontract::{H256, U256};
#[cfg(test)]
use mockall::automock;
use thiserror::Error;

#[cfg_attr(test, automock)]
pub trait StableXSolutionSubmitting {
    /// Return the objective value for the given solution in the given
    /// batch or an error.
    ///
    /// # Arguments
    /// * `batch_index` - the auction for which this solutions should be evaluated
    /// * `orders` - the list of orders for which this solution is applicable
    /// * `solution` - the solution to be evaluated
    fn get_solution_objective_value(
        &self,
        batch_index: U256,
        orders: Vec<Order>,
        solution: Solution,
    ) -> Result<U256, SolutionSubmissionError>;

    /// Submits the provided solution and returns the result of the submission
    ///
    /// # Arguments
    /// * `batch_index` - the auction for which this solutions should be evaluated
    /// * `orders` - the list of orders for which this solution is applicable
    /// * `solution` - the solution to be evaluated
    /// * `claimed_objective_value` - the objective value of the provided solution.
    fn submit_solution(
        &self,
        batch_index: U256,
        orders: Vec<Order>,
        solution: Solution,
        claimed_objective_value: U256,
    ) -> Result<(), SolutionSubmissionError>;
}

/// An error with verifying or submitting a solution
#[derive(Debug, Error)]
pub enum SolutionSubmissionError {
    #[error("Benign Error: {0}")]
    Benign(String),
    #[error("Unexpected Error: {0}")]
    Unexpected(Error),
}

impl From<Error> for SolutionSubmissionError {
    fn from(err: Error) -> Self {
        err.downcast_ref::<MethodError>()
            .and_then(|method_error| match &method_error.inner {
                ExecutionError::Revert(Some(reason)) => {
                    let reason_slice: &str = &*reason;
                    match reason_slice {
                        "New objective doesn\'t sufficiently improve current solution" => {
                            Some(SolutionSubmissionError::Benign(reason.clone()))
                        }
                        "Claimed objective doesn't sufficiently improve current solution" => {
                            Some(SolutionSubmissionError::Benign(reason.clone()))
                        }
                        "SafeMath: subtraction overflow" => {
                            Some(SolutionSubmissionError::Benign(reason.clone()))
                        }
                        _ => None,
                    }
                }
                _ => None,
            })
            .unwrap_or_else(|| SolutionSubmissionError::Unexpected(err))
    }
}

pub struct StableXSolutionSubmitter<'a> {
    contract: &'a dyn StableXContract,
    web3: &'a contracts::Web3,
}

impl<'a> StableXSolutionSubmitter<'a> {
    pub fn new(contract: &'a dyn StableXContract, web3: &'a contracts::Web3) -> Self {
        Self { contract, web3 }
    }
}

impl<'a> StableXSolutionSubmitting for StableXSolutionSubmitter<'a> {
    fn get_solution_objective_value(
        &self,
        batch_index: U256,
        orders: Vec<Order>,
        solution: Solution,
    ) -> Result<U256, SolutionSubmissionError> {
        self.contract
            .get_solution_objective_value(batch_index, orders, solution, None)
            .map_err(SolutionSubmissionError::from)
    }

    fn submit_solution(
        &self,
        batch_index: U256,
        orders: Vec<Order>,
        solution: Solution,
        claimed_objective_value: U256,
    ) -> Result<(), SolutionSubmissionError> {
        self.contract
            .submit_solution(
                batch_index.clone(),
                orders.clone(),
                solution.clone(),
                claimed_objective_value,
            )
            .map_err(|err| {
                extract_transaction_hash(&err)
                    .and_then(|hash| {
                        let receipt = self.web3.eth().transaction_receipt(hash).wait().ok()??;
                        let block_number = receipt.block_number?;
                        match self.contract.get_solution_objective_value(
                            batch_index,
                            orders,
                            solution,
                            Some(block_number.into()),
                        ) {
                            Ok(_) => None,
                            Err(e) => Some(SolutionSubmissionError::from(e)),
                        }
                    })
                    .unwrap_or_else(|| SolutionSubmissionError::Unexpected(err))
            })
    }
}

fn extract_transaction_hash(err: &Error) -> Option<H256> {
    err.downcast_ref::<MethodError>()
        .and_then(|method_error| match &method_error.inner {
            ExecutionError::Failure(tx_hash) => Some(*tx_hash),
            _ => None,
        })
}
