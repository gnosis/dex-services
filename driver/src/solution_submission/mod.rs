#![allow(clippy::ptr_arg)] // required for automock

use crate::contracts::stablex_contract::StableXContract;
use crate::eth_rpc::EthRpc;
use crate::models::Solution;

use anyhow::{Error, Result};
use ethcontract::errors::{ExecutionError, MethodError};
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
    rpc: &'a dyn EthRpc,
}

impl<'a> StableXSolutionSubmitter<'a> {
    pub fn new(contract: &'a dyn StableXContract, rpc: &'a dyn EthRpc) -> Self {
        Self { contract, rpc }
    }
}

impl<'a> StableXSolutionSubmitting for StableXSolutionSubmitter<'a> {
    fn get_solution_objective_value(
        &self,
        batch_index: U256,
        solution: Solution,
    ) -> Result<U256, SolutionSubmissionError> {
        self.contract
            .get_solution_objective_value(batch_index, solution, None)
            .map_err(SolutionSubmissionError::from)
    }

    fn submit_solution(
        &self,
        batch_index: U256,
        solution: Solution,
        claimed_objective_value: U256,
    ) -> Result<(), SolutionSubmissionError> {
        self.contract
            .submit_solution(
                batch_index.clone(),
                solution.clone(),
                claimed_objective_value,
            )
            .map_err(|err| {
                extract_transaction_hash(&err)
                    .and_then(|hash| {
                        let receipt = self.rpc.get_transaction_receipts(hash).ok()??;
                        let block_number = receipt.block_number?;
                        match self.contract.get_solution_objective_value(
                            batch_index,
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::contracts::stablex_contract::MockStableXContract;
    use crate::eth_rpc::MockEthRpc;

    use anyhow::anyhow;
    use ethcontract::web3::types::{TransactionReceipt, H2048};
    use mockall::predicate::{always, eq};

    #[test]
    fn test_benign_verification_failure() {
        let eth_rpc = MockEthRpc::new();
        let mut contract = MockStableXContract::new();

        contract
            .expect_get_solution_objective_value()
            .return_once(move |_, _, _| {
                Err(anyhow!(MethodError::from_parts(
                "submitSolution(uint32,uint256,address[],uint16[],uint128[],uint128[],uint16[])"
                    .to_owned(),
                ExecutionError::Revert(Some("SafeMath: subtraction overflow".to_owned())),
            )))
            });

        let submitter = StableXSolutionSubmitter::new(&contract, &eth_rpc);
        let result = submitter.get_solution_objective_value(U256::zero(), Solution::trivial());

        match result.expect_err("Should have errored") {
            SolutionSubmissionError::Benign(_) => (),
            SolutionSubmissionError::Unexpected(err) => {
                panic!("Expecting benign failure, but got {}", err)
            }
        };
    }

    #[test]
    fn test_benign_solution_submission_failure() {
        let mut eth_rpc = MockEthRpc::new();
        let mut contract = MockStableXContract::new();

        let tx_hash = H256::zero();
        let block_number = 42.into();
        let receipt = TransactionReceipt {
            transaction_hash: tx_hash,
            transaction_index: 0.into(),
            block_hash: None,
            block_number: Some(block_number),
            cumulative_gas_used: U256::zero(),
            gas_used: None,
            contract_address: None,
            logs: vec![],
            status: None,
            logs_bloom: H2048::zero(),
        };

        eth_rpc
            .expect_get_transaction_receipts()
            .with(eq(tx_hash))
            .return_const(Ok(Some(receipt)));

        // Submit Solution returns failed tx
        contract
            .expect_submit_solution()
            .return_once(move |_, _, _| {
                Err(anyhow!(MethodError::from_parts(
                "submitSolution(uint32,uint256,address[],uint16[],uint128[],uint128[],uint16[])"
                    .to_owned(),
                ExecutionError::Failure(tx_hash),
            )))
            });
        // Get objective value on old block number returns revert reason
        contract
            .expect_get_solution_objective_value()
            .with(always(), always(), eq(Some(block_number.into())))
            .return_once(move |_, _, _| {
                Err(anyhow!(MethodError::from_parts(
                "submitSolution(uint32,uint256,address[],uint16[],uint128[],uint128[],uint16[])"
                    .to_owned(),
                ExecutionError::Revert(Some("Claimed objective doesn't sufficiently improve current solution".to_owned())),
            )))
            });

        let submitter = StableXSolutionSubmitter::new(&contract, &eth_rpc);
        let result = submitter.submit_solution(U256::zero(), Solution::trivial(), U256::zero());

        match result.expect_err("Should have errored") {
            SolutionSubmissionError::Benign(_) => (),
            SolutionSubmissionError::Unexpected(err) => {
                panic!("Expecting benign failure, but got {}", err)
            }
        };
    }
}
