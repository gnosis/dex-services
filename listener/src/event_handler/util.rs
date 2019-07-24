use std::sync::Arc;
use graph::components::store::{EntityFilter, EntityKey, EntityQuery, EntityRange};
use graph::data::store::Entity;
use web3::types::Log;

use crate::SUBGRAPH_ID;

pub fn entity_id_from_log(log: &Arc<Log>) -> String {
    format!("{:x}_{}", 
        &log.block_hash.unwrap(), 
        &log.log_index.unwrap()
    )
}

pub fn entity_key(entity_type: &str, entity: &Entity) -> EntityKey {
    EntityKey {
        subgraph_id: SUBGRAPH_ID.clone(),
        entity_type: entity_type.to_string(),
        entity_id: entity.get("id")
            .and_then(|v| v.clone().as_string())
            .unwrap(),
    }
}

pub fn entity_query(entity_type: &str, filter: EntityFilter) -> EntityQuery {
    EntityQuery {
        subgraph_id: SUBGRAPH_ID.clone(),
        entity_types: vec![entity_type.to_string()],
        filter: Some(filter),
        order_by: None,
        order_direction: None,
        range: EntityRange {
            first: None,
            skip: 0
        }
    }
}

#[cfg(test)]
pub mod test {
    use super::*;
    use graph::components::ethereum::EthereumBlock;
    use graph::data::schema::Schema;
    use slog::Logger;
    use web3::types::{Block, Bytes, H2048, H256, H160, Transaction, U256};

    pub fn logger() -> Logger {
        Logger::root(slog::Discard, o!())
    }

    pub fn fake_schema() -> Schema {
        Schema::parse("scalar Foo", SUBGRAPH_ID.clone()).unwrap()
    }

    pub fn fake_tx() -> Transaction {
        Transaction {
            hash: H256::zero(),
            nonce: U256::zero(),
            block_hash: None,
            block_number: None,
            transaction_index: None,
            from: H160::zero(),
            to: None,
            value: U256::zero(),
            gas_price: U256::zero(),
            gas: U256::zero(),
            input: Bytes(vec![]),
        }
    }

    pub fn fake_block() -> EthereumBlock{
        EthereumBlock {
            block: Block {
                hash: None,
                parent_hash: H256::zero(),
                uncles_hash: H256::zero(),
                author: H160::zero(),
                state_root: H256::zero(),
                transactions_root: H256::zero(),
                receipts_root: H256::zero(),
                number: None,
                gas_used: U256::zero(),
                gas_limit: U256::zero(),
                extra_data: Bytes(vec![]),
                logs_bloom: H2048::zero(),
                timestamp: U256::zero(),
                difficulty: U256::zero(),
                total_difficulty: U256::zero(),
                seal_fields: vec![],
                uncles: vec![],
                transactions: vec![],
                size: None,
                mix_hash: None,
                nonce: None,
            },
            transaction_receipts: vec![],
        }
    }
}