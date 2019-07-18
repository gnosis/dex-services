use failure::Error;
use futures::future::*;
use slog::Logger;
use std::sync::Arc;

use graph::components::ethereum::{EthereumCall, EthereumBlockTriggerType, EthereumBlock};
use graph::components::subgraph::{RuntimeHost as RuntimeHostTrait, RuntimeHostBuilder, BlockState};

use graph::data::subgraph::{DataSource, SubgraphDeploymentId};

use web3::types::{Log, Transaction};

#[derive(Clone, Debug)]
pub struct RustRuntimeHost {}

impl RuntimeHostBuilder for RustRuntimeHost {
    type Host = RustRuntimeHost;
    fn build(
        &self,
        _logger: &Logger,
        _subgraph_id: SubgraphDeploymentId,
        _data_source: DataSource,
    ) -> Result<Self::Host, Error> {
        Ok(RustRuntimeHost {})
    }
}

impl RuntimeHostTrait for RustRuntimeHost {
    fn matches_log(&self, _log: &Log) -> bool {
        unimplemented!();
    }

    fn matches_call(&self, _call: &EthereumCall) -> bool {
        unimplemented!();
    }

    /// Returns true if the RuntimeHost has a handler for an Ethereum block.
    fn matches_block(&self, _call: EthereumBlockTriggerType) -> bool {
        unimplemented!();
    }

    /// Process an Ethereum event and return a vector of entity operations.
    fn process_log(
        &self,
        _logger: Logger,
        _block: Arc<EthereumBlock>,
        _transaction: Arc<Transaction>,
        _log: Arc<Log>,
        _state: BlockState,
    ) -> Box<Future<Item = BlockState, Error = Error> + Send> {
        unimplemented!();
    }

    /// Process an Ethereum call and return a vector of entity operations
    fn process_call(
        &self,
        _logger: Logger,
        _block: Arc<EthereumBlock>,
        _transaction: Arc<Transaction>,
        _call: Arc<EthereumCall>,
        _state: BlockState,
    ) -> Box<Future<Item = BlockState, Error = Error> + Send> {
        unimplemented!();
    }

    /// Process an Ethereum block and return a vector of entity operations
    fn process_block(
        &self,
        _logger: Logger,
        _block: Arc<EthereumBlock>,
        _trigger_type: EthereumBlockTriggerType,
        _state: BlockState,
    ) -> Box<Future<Item = BlockState, Error = Error> + Send> {
        unimplemented!();
    }
}