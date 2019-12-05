use failure::Error;
use futures::future::*;
use slog::{info, warn, Logger};
use std::collections::HashMap;
use std::sync::Arc;

use dfusion_core::database::DbInterface;

use futures::sync::mpsc::Sender;

use graph::components::ethereum::{EthereumBlockTriggerType, EthereumCall, LightEthereumBlock};
use graph::components::subgraph::{
    BlockState, HostMetrics, RuntimeHost as RuntimeHostTrait, RuntimeHostBuilder,
};
use graph::data::subgraph::{DataSource, DataSourceTemplate, SubgraphDeploymentId};

use tiny_keccak::keccak256;

use web3::types::{Log, Transaction, H256};

use crate::event_handler::{
    AuctionSettlementHandler, DepositHandler, EventHandler, FluxTransitionHandler,
    InitializationHandler, SellOrderHandler, StandingOrderHandler, WithdrawHandler,
};

type HandlerMap = HashMap<H256, Box<dyn EventHandler>>;

fn register_event(handlers: &mut HandlerMap, name: &str, handler: Box<dyn EventHandler>) {
    handlers.insert(H256::from(keccak256(name.as_bytes())), handler);
}

#[derive(Clone)]
pub struct RustRuntimeHostBuilder {
    store: Arc<dyn DbInterface>,
}

impl RustRuntimeHostBuilder {
    pub fn new(store: Arc<dyn DbInterface>) -> Self {
        RustRuntimeHostBuilder { store }
    }
}

impl RuntimeHostBuilder for RustRuntimeHostBuilder {
    type Req = ();
    type Host = RustRuntimeHost;

    fn build(
        &self,
        _network_name: String,
        _subgraph_id: SubgraphDeploymentId,
        _data_source: DataSource,
        _top_level_templates: Vec<DataSourceTemplate>,
        _mapping_request_sender: Sender<Self::Req>,
        _metrics: Arc<HostMetrics>,
    ) -> Result<Self::Host, Error> {
        Ok(RustRuntimeHost::new(self.store.clone()))
    }

    fn spawn_mapping(
        _parsed_module: parity_wasm::elements::Module,
        _logger: Logger,
        _subgraph_id: SubgraphDeploymentId,
        _metrics: Arc<HostMetrics>,
    ) -> Result<Sender<Self::Req>, Error> {
        unimplemented!();
    }
}

#[derive(Debug)]
pub struct RustRuntimeHost {
    handlers: HashMap<H256, Box<dyn EventHandler>>,
}

impl RustRuntimeHost {
    fn new(store: Arc<dyn DbInterface>) -> Self {
        let mut handlers = HashMap::new();
        register_event(
            &mut handlers,
            "Deposit(uint16,uint8,uint128,uint256,uint16)",
            Box::new(DepositHandler {}),
        );
        register_event(
            &mut handlers,
            "SnappInitialization(bytes32,uint8,uint16)",
            Box::new(InitializationHandler {}),
        );
        register_event(
            &mut handlers,
            "StateTransition(uint8,uint256,bytes32,uint256)",
            Box::new(FluxTransitionHandler::new(store.clone())),
        );
        register_event(
            &mut handlers,
            "WithdrawRequest(uint16,uint8,uint128,uint256,uint16)",
            Box::new(WithdrawHandler {}),
        );
        register_event(
            &mut handlers,
            "StandingSellOrderBatch(uint256,uint256,uint16,bytes)",
            Box::new(StandingOrderHandler {}),
        );
        register_event(
            &mut handlers,
            "SellOrder(uint256,uint16,uint16,uint8,uint8,uint96,uint96)",
            Box::new(SellOrderHandler {}),
        );
        register_event(
            &mut handlers,
            "AuctionSettlement(uint256,uint256,bytes32,bytes)",
            Box::new(AuctionSettlementHandler::new(store.clone())),
        );
        RustRuntimeHost { handlers }
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
    fn matches_block(
        &self,
        _block_trigger_type: EthereumBlockTriggerType,
        _block_number: u64,
    ) -> bool {
        unimplemented!();
    }

    /// Process an Ethereum event and return a vector of entity operations.
    fn process_log(
        &self,
        logger: Logger,
        block: Arc<LightEthereumBlock>,
        transaction: Arc<Transaction>,
        log: Arc<Log>,
        state: BlockState,
    ) -> Box<dyn Future<Item = BlockState, Error = Error> + Send> {
        info!(logger, "Received event");
        let mut state = state;
        if let Some(handler) = self.handlers.get(&log.topics[0]) {
            match handler.process_event(logger, block, transaction, log) {
                Ok(ops) => state.entity_cache.append(ops),
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
        _block: Arc<LightEthereumBlock>,
        _transaction: Arc<Transaction>,
        _call: Arc<EthereumCall>,
        _state: BlockState,
    ) -> Box<dyn Future<Item = BlockState, Error = Error> + Send> {
        unimplemented!();
    }

    /// Process an Ethereum block and return a vector of entity operations
    fn process_block(
        &self,
        _logger: Logger,
        _block: Arc<LightEthereumBlock>,
        _trigger_type: EthereumBlockTriggerType,
        _state: BlockState,
    ) -> Box<dyn Future<Item = BlockState, Error = Error> + Send> {
        unimplemented!();
    }
}
