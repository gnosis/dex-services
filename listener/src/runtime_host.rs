use failure::Error;
use futures::future::*;
use slog::Logger;
use std::collections::HashMap;
use std::sync::Arc;

use graph::components::ethereum::{EthereumCall, EthereumBlockTriggerType, EthereumBlock};
use graph::components::subgraph::{RuntimeHost as RuntimeHostTrait, RuntimeHostBuilder, BlockState};

use graph::data::subgraph::{DataSource, SubgraphDeploymentId};

use web3::types::{Log, Transaction, H256};

use crate::event_handler::EventHandler;

#[derive(Clone)]
pub struct RustRuntimeHostBuilder {}
impl RuntimeHostBuilder for RustRuntimeHostBuilder {
    type Host = RustRuntimeHost;
    fn build(
        &self,
        _logger: &Logger,
        _subgraph_id: SubgraphDeploymentId,
        _data_source: DataSource,
    ) -> Result<Self::Host, Error> {
        Ok(RustRuntimeHost::new())
    }
}

#[derive(Debug)]
pub struct RustRuntimeHost {
    handlers: HashMap<H256, Box<EventHandler>>
}

impl RustRuntimeHost {
    fn new() -> Self {
        RustRuntimeHost {
            handlers: HashMap::new()
        }
    }
}

impl RuntimeHostTrait for RustRuntimeHost {
    fn matches_log(&self, _log: &Log) -> bool {
        true
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
        logger: Logger,
        block: Arc<EthereumBlock>,
        transaction: Arc<Transaction>,
        log: Arc<Log>,
        state: BlockState,
    ) -> Box<Future<Item = BlockState, Error = Error> + Send> {
        info!(logger, "Received event");
        let mut state = state;
        if let Some(handler) = self.handlers.get(&log.topics[0]) {
            match handler.process_event(logger, block, transaction, log) {
                Ok(mut ops) => state.entity_operations.append(&mut ops),
                Err(e) => return Box::new(err(e)),
            }
        } else {
            warn!(logger, "Unhandled event with topic {}", &log.topics[0]);
        };
        Box::new(ok(state))
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