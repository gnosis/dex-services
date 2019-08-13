use crate::models;
use crate::models::{ConcatenatingHashable, RollingHashable};

use array_macro::array;
use serde_derive::Deserialize;
use sha2::{Digest, Sha256};
use std::sync::Arc;
use web3::types::{Log};
use web3::types::{H256, U256};
use graph::data::store::{Entity};

use super::util::*;

#[derive(Debug, Clone, Deserialize, Eq, Ord, PartialEq, PartialOrd)]
#[serde(rename_all = "camelCase")]
pub struct StandingOrder {
    pub account_id: u16,
    pub batch_index: U256,
    pub valid_from_auction_id: U256,
    orders: Vec<super::Order>,
}

impl StandingOrder {
    pub fn new(
        account_id: u16,
        batch_index: U256,
        valid_from_auction_id: U256,
        orders: Vec<super::Order>,
    ) -> StandingOrder {
        StandingOrder {
            account_id,
            batch_index,
            valid_from_auction_id,
            orders,
        }
    }

    pub fn empty_array() -> [models::StandingOrder; models::NUM_RESERVED_ACCOUNTS] {
        let mut i = 0u16;
        array![models::StandingOrder::empty({i += 1; i - 1}); models::NUM_RESERVED_ACCOUNTS]
    }
    pub fn get_orders(&self) -> &Vec<super::Order> {
        &self.orders
    }
    pub fn num_orders(&self) -> usize {
        self.orders.len()
    }
    pub fn empty(account_id: u16) -> StandingOrder {
        models::StandingOrder::new(account_id, U256::zero(), U256::zero(), vec![])
    }
}

impl ConcatenatingHashable for [models::StandingOrder; models::NUM_RESERVED_ACCOUNTS] {
    fn concatenating_hash(&self, init_hash: H256) -> H256 {
        let mut hasher = Sha256::new();
        hasher.input(init_hash);
        self.iter()
            .for_each(|k| hasher.input(k.get_orders().rolling_hash(0)));
        let result = hasher.result();
        let b: Vec<u8> = result.to_vec();
        H256::from(b.as_slice())
    }
}

impl From<mongodb::ordered::OrderedDocument> for StandingOrder {
    fn from(document: mongodb::ordered::OrderedDocument) -> Self {
        let account_id = document.get_i32("_id").unwrap() as u16;
        let batch_index = U256::from(document.get_i32("batchIndex").unwrap());
        let valid_from_auction_id = U256::from(document.get_i32("validFromAuctionId").unwrap());
        StandingOrder {
            account_id,
            batch_index,
            valid_from_auction_id,
            orders: document
                .get_array("orders")
                .unwrap()
                .iter()
                .map(|raw_order| raw_order.as_document().unwrap())
                .map(|order_doc| super::Order {
                        batch_information: None,
                        account_id,
                        buy_token: order_doc.get_i32("buyToken").unwrap() as u8,
                        sell_token: order_doc.get_i32("sellToken").unwrap() as u8,
                        buy_amount: order_doc.get_str("buyAmount").unwrap().parse().unwrap(),
                        sell_amount: order_doc.get_str("sellAmount").unwrap().parse().unwrap(),
                    }
                ).collect()
        }
    }
}

impl From<&Arc<Log>> for StandingOrder {
    fn from(log: &Arc<Log>) -> Self {
        let mut bytes: Vec<u8> = log.data.0.clone();
        info!("Parsing StandingOrder from bytes. {} bytes. {:?}", bytes.len(), bytes);
        
        // Get basic data from event
        let batch_index = U256::pop_from_log_data(&mut bytes);
        let valid_from_auction_id = U256::pop_from_log_data(&mut bytes);
        let account_id = u16::pop_from_log_data(&mut bytes);

        let bytes_init = u8::pop_from_log_data(&mut bytes) as usize;
        let byte_size = u8::pop_from_log_data(&mut bytes) as usize;

        info!("Extracting packed order. Bytes: {}-{}", bytes_init, bytes_init + byte_size);
        let packed_orders_bytes = &bytes[0..byte_size];
        info!("Parsing orders from packedOrders. {} bytes. {:?}", packed_orders_bytes.len(), packed_orders_bytes);
        assert!(packed_orders_bytes.len() % 26 == 0, "Each order should be packed in 26 bytes");
                
        // Extract packed order info
        let orders: Vec<models::Order> = packed_orders_bytes
            .chunks(26)
            .map(|chunk| models::Order::from_encoded_order(account_id, chunk.to_vec()))
            .collect();

        StandingOrder {
            account_id,
            batch_index,
            valid_from_auction_id,
            orders
        }
    }
}


impl From<Entity> for StandingOrder {
    fn from(entity: Entity) -> Self {
        let account_id = u16::from_entity(&entity, "accountId");
        let batch_index = U256::from_entity(&entity, "batchIndex");
        let valid_from_auction_id = U256::from_entity(&entity, "validFromAuctionId");

        StandingOrder {
            account_id,
            batch_index,
            valid_from_auction_id,            
            orders: vec![] // Orders cannot be reconstructed just from the entity
        }
    }
}

impl Into<Entity> for StandingOrder {
    fn into(self) -> Entity {
        let mut entity = Entity::new();
                
        entity.set("accountId", self.account_id.to_value());
        entity.set("batchIndex", self.batch_index.to_value());
        entity.set("validFromAuctionId", self.valid_from_auction_id.to_value());
        entity.set("orders", vec![]);

        
        entity
    }
}


#[cfg(test)]
pub mod tests {
    use super::*;
    use std::str::FromStr;
    use graph::bigdecimal::BigDecimal;
    use web3::types::{Bytes, H256};

    #[test]
    fn concatenating_hash() {
        let standing_order = models::StandingOrder::new(
            1,
            U256::zero(),
            U256::zero(),
            vec![create_order_for_test(), create_order_for_test()],
        );
        let mut standing_orders = models::StandingOrder::empty_array();
        standing_orders[1] = standing_order;
        assert_eq!(
            standing_orders.concatenating_hash(H256::from(0)),
            H256::from_str("6bdda4f03645914c836a16ba8565f26dffb7bec640b31e1f23e0b3b22f0a64ae")
                .unwrap()
        );
    }

    #[test]
    fn from_log() {
        let log = create_log_for_test();
        let expected_standing_order = create_standing_order_for_test();

        let actual_standing_order = StandingOrder::from(&log);
        assert_eq!(actual_standing_order, expected_standing_order);
    }

    #[test]
    fn into_entity() {
        let standing_order = create_standing_order_for_test();
        let entity = create_entity_for_test();

        assert_eq!(entity, standing_order.clone().into());
    }

    #[test]
    fn from_entity() {
        let entity = create_entity_for_test();
        let mut standing_order = create_standing_order_for_test();

        // Remove the orders, the From<Entity> doesn't have enough info about the orders
        standing_order.orders = vec![]; 

        assert_eq!(standing_order, StandingOrder::from(entity));
    }

    pub fn create_standing_order_for_test() -> models::StandingOrder {
        StandingOrder {
            account_id: 1,
            batch_index: U256::from(2),
            valid_from_auction_id: U256::from(3),
            orders: vec![models::Order {
                batch_information: None,
                account_id: 1,
                sell_token: 1,
                buy_token: 2,
                sell_amount: 2 * (10 as u128).pow(18),
                buy_amount: (10 as u128).pow(18)
            }]
        }
    }

    pub fn create_entity_for_test() -> Entity {
        let mut entity = Entity::new();
        entity.set("accountId", 1);
        entity.set("batchIndex", BigDecimal::from(2));
        entity.set("validFromAuctionId", BigDecimal::from(3));
        entity.set("orders", vec![]);

        entity
    }

    pub fn create_order_for_test() -> models::Order {
        models::Order {
            batch_information: None,
            account_id: 1,
            sell_token: 2,
            buy_token: 3,
            sell_amount: 4,
            buy_amount: 5,
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
