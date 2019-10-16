use graph::components::store::EntityKey;
use graph::data::store::Entity;
use std::sync::Arc;
use web3::types::Log;

use crate::SUBGRAPH_ID;

pub fn entity_id_from_log(log: &Arc<Log>) -> String {
    format!("{:x}_{}", &log.block_hash.unwrap(), &log.log_index.unwrap())
}

pub fn entity_key(entity_type: &str, entity: &Entity) -> EntityKey {
    EntityKey {
        subgraph_id: SUBGRAPH_ID.clone(),
        entity_type: entity_type.to_string(),
        entity_id: entity
            .get("id")
            .and_then(|v| v.clone().as_string())
            .unwrap(),
    }
}

#[cfg(test)]
pub mod test {
    use graph::components::ethereum::EthereumBlock;
    use slog::{o, Discard, Logger};
    use web3::types::{Block, Bytes, Transaction, H160, H2048, H256, U256};

    pub fn logger() -> Logger {
        Logger::root(Discard, o!())
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

    pub fn fake_block() -> EthereumBlock {
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
