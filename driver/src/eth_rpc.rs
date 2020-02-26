use crate::contracts;

use ethcontract::web3::error::Error;
use ethcontract::web3::futures::Future;
use ethcontract::web3::types::TransactionReceipt;
use ethcontract::H256;
#[cfg(test)]
use mockall::automock;

/// Interface defining possible interactions via Ethereum RPCs
/// Main purpose it to allow mocking the concrete Web3 implementation
#[cfg_attr(test, automock)]
pub trait EthRpc {
    fn get_transaction_receipts(&self, tx_hash: H256) -> Result<Option<TransactionReceipt>, Error>;
}

/// Ethereum RPC implementation via Web3
pub struct Web3EthRpc<'a> {
    web3: &'a contracts::Web3,
}

impl<'a> Web3EthRpc<'a> {
    pub fn new(web3: &'a contracts::Web3) -> Self {
        Self { web3 }
    }
}

impl<'a> EthRpc for Web3EthRpc<'a> {
    fn get_transaction_receipts(&self, tx_hash: H256) -> Result<Option<TransactionReceipt>, Error> {
        self.web3.eth().transaction_receipt(tx_hash).wait()
    }
}
