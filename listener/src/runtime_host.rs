use failure::Error;
use futures::future::*;
use slog::Logger;
use std::collections::HashMap;
use std::sync::Arc;

use dfusion_core::database::DbInterface;

use graph::components::ethereum::{EthereumCall, EthereumBlockTriggerType, EthereumBlock};
use graph::components::subgraph::{RuntimeHost as RuntimeHostTrait, RuntimeHostBuilder, BlockState};

use graph::data::subgraph::{DataSource, SubgraphDeploymentId};

use tiny_keccak::keccak256;

use web3::types::{Log, Transaction, H256};

use crate::event_handler::{EventHandler, DepositHandler, InitializationHandler, FluxTransitionHandler, WithdrawHandler, StandingOrderHandler, SellOrderHandler };

type HandlerMap = HashMap<H256, Box<dyn EventHandler>>;

fn register_event(handlers: &mut HandlerMap, name: &str, handler: Box<dyn EventHandler>) 
{
    handlers.insert(
        H256::from(keccak256(name.as_bytes())),
        handler
    );
}

#[derive(Clone)]
pub struct RustRuntimeHostBuilder {
    store: Arc<DbInterface>
}

impl RustRuntimeHostBuilder {
    pub fn new(store: Arc<DbInterface>) -> Self {
        RustRuntimeHostBuilder {
            store
        }
    }
}

impl RuntimeHostBuilder for RustRuntimeHostBuilder {
    type Host = RustRuntimeHost;
    fn build(
        &self,
        _logger: &Logger,
        _subgraph_id: SubgraphDeploymentId,
        _data_source: DataSource,
    ) -> Result<Self::Host, Error> {
        Ok(RustRuntimeHost::new(self.store.clone()))
    }
}

#[derive(Debug)]
pub struct RustRuntimeHost {
    handlers: HashMap<H256, Box<dyn EventHandler>>
}

impl RustRuntimeHost {
    fn new(store: Arc<DbInterface>) -> Self {
        let mut handlers = HashMap::new();
        register_event(
            &mut handlers,
            "Deposit(uint16,uint8,uint128,uint256,uint16)",
            Box::new(DepositHandler {})
        );
        register_event(
            &mut handlers,
            "SnappInitialization(bytes32,uint8,uint16)",
            Box::new(InitializationHandler {})
        );
        register_event(
            &mut handlers,
            "StateTransition(uint8,uint256,bytes32,uint256)",
            Box::new(FluxTransitionHandler::new(store))
        );
        register_event(
            &mut handlers,
            "WithdrawRequest(uint16,uint8,uint128,uint256,uint16)",
            Box::new(WithdrawHandler {})
        );
        register_event(
            &mut handlers,
            "StandingSellOrderBatch(uint256,uint256,uint16,bytes)",
            Box::new(StandingOrderHandler {})
        );
        register_event(
            &mut handlers,
            "SellOrder(uint256,uint16,uint16,uint8,uint8,uint96,uint96)",
            Box::new(SellOrderHandler {})
        );
        RustRuntimeHost {
            handlers
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