use std::sync::Arc;
use graph::components::store::EntityKey;
use web3::types::Log;

use crate::SUBGRAPH_ID;

pub fn entity_id_from_log(log: &Arc<Log>) -> String {
    format!("{:x}_{}", 
        &log.block_hash.unwrap(), 
        &log.log_index.unwrap()
    )
}

pub fn entity_key(entity_type: &str, entity_id: &str) -> EntityKey {
    EntityKey {
        subgraph_id: SUBGRAPH_ID.clone(),
        entity_type: entity_type.to_string(),
        entity_id: entity_id.to_string(),
    }
}