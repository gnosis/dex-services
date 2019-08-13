use failure::Error;
use super::EventHandler;
use slog::Logger;
use std::sync::Arc;

use graph::components::ethereum::EthereumBlock;
use graph::components::store::{EntityOperation};
use graph::data::store::{Entity, Value};

use dfusion_core::models::StandingOrder;

use web3::types::{Log, Transaction};

use super::util;

#[derive(Debug, Clone)]
pub struct StandingOrderHandler {}

impl EventHandler for StandingOrderHandler {
    fn process_event(
        &self,
        logger: Logger,
        _block: Arc<EthereumBlock>,
        _transaction: Arc<Transaction>,
        log: Arc<Log>
    ) -> Result<Vec<EntityOperation>, Error> {
        // Get entities from log
        let (standing_order_entity, order_entities) = get_entities_from_log(&logger, &log);

        // Define operations to persist orders
        let mut entity_operations: Vec<EntityOperation> = order_entities.into_iter()
            .map(|order| {
                EntityOperation::Set {
                    key: util::entity_key("SellOrder", &order),
                    data: order
                }
            })
            .collect();

        // Add operation to persist the standing order
        entity_operations.push(EntityOperation::Set {
            key: util::entity_key("StandingSellOrderBatch", &standing_order_entity),
            data: standing_order_entity
        });

        Ok(entity_operations)
    }
}

fn get_entities_from_log(logger: &Logger, log: &Arc<Log>) -> (Entity, Vec<Entity>) {
    let standing_order = StandingOrder::from(log);
    let orders = standing_order.get_orders().clone();
    let entity_id = util::entity_id_from_log(&log);
    info!(logger, "Processing StandingOrder batch. Id: {}. Data: {:?}", &entity_id, &standing_order);

    // Get standing order entity and set id
    let mut standing_order_entity: Entity = standing_order.into();
    standing_order_entity.set("id", &entity_id);

    // Get order entities and their ids
    let order_entities: Vec<Entity> = orders.into_iter()
        .enumerate()
        .map(|(order_number, order)| {
            // Transform order into Entity and add id
            let mut order_entity: Entity = order.into();
            order_entity.set("id", format!("{}_{}", &entity_id, &order_number));

            order_entity
        })
        .collect();

    // Add order entity ids
    let order_entities_ids: Vec<Value> = order_entities
        .iter()
        .map(|order| order.get("id").unwrap().clone())
        .collect();
    standing_order_entity.set("orders", order_entities_ids);

    (standing_order_entity, order_entities)
}



#[cfg(test)]
pub mod test {
    use super::*;
    use web3::types::{Bytes};
    use graph::components::store::EntityKey;

    #[test]
    fn test_handle_standing_order() {
        let handler = StandingOrderHandler{};
        let log = create_log_for_test();

        let result = handler.process_event(
            util::test::logger(), 
            Arc::new(util::test::fake_block()),
            Arc::new(util::test::fake_tx()),
            log
        );

        let operations: Vec<EntityOperation> = result.expect("The handler should succeed");
        assert_eq!(operations.len(), 2, "Expected exactly two operations");
        let expected_entity_types = ["SellOrder", "StandingSellOrderBatch"];
        
        // Check that the two operations are persisting a SellOrder and a StandingSellOrderBatch
        for (operation, expected_entity_type) in operations.iter().zip(expected_entity_types.iter()) {
            match operation {
                EntityOperation::Set { key: EntityKey {
                    subgraph_id: _subgraph_id,
                    entity_type,
                    entity_id: _entity_id
                }, data: _data} => {
                    assert_eq!(entity_type, expected_entity_type);
                },
                _ => panic!("Expected a Set Entity operation")
            }
        }
    }

    pub fn create_log_for_test() -> Arc<Log> {
        let bytes: Vec<Vec<u8>> = vec![
            // batch_index: 1
            vec![0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,0, 0, 0, 2],

            // valid_from_auction_id: 3
            vec![0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 3],

            // account_id: 1
            vec![0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1],

            // bytes start position
            vec![0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 128],

            // bytes size
            vec![0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 26],

            // packed_orders: Buy token 2, for token 1. Buy 1e18 for 2e18.
            //    000000000de0b6b3a7640000 000000001bc16d674ec80000 0201
            //    00 00 00 00 0d  e0   b6   b3   a7   64   00 00 00 00 00 00 1b  c1   6d   67   4e  c8   00 00 02 01
            vec![ 0, 0, 0, 0, 13, 224, 182, 179, 167, 100, 0, 0, 0, 0, 0, 0, 27, 193, 109, 103, 78, 200, 0, 0, 2, 1],

            // Unused space for bytes field
            vec![ 0, 0, 0, 0, 0, 0]
        ];

        Arc::new(Log {
            address: 0.into(),
            topics: vec![],
            data: Bytes(bytes.iter().flat_map(|i| i.iter()).cloned().collect()),
            block_hash: Some(2.into()),
            block_number: Some(1.into()),
            transaction_hash: Some(3.into()),
            transaction_index: Some(0.into()),
            log_index: Some(0.into()),
            transaction_log_index: Some(0.into()),
            log_type: None,
            removed: None,
        })
    }
}