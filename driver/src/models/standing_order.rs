use serde_derive::{Deserialize};
use sha2::{Digest, Sha256};
use web3::types::H256;
use crate::models::{ConcatingHashable, RollingHashable};
use crate::models;

#[derive(Debug, Clone, Deserialize, Eq, Ord, PartialEq, PartialOrd)]
#[serde(rename_all = "camelCase")]
pub struct StandingOrder {
    pub account_id: u16,
    pub batch_index: u32,
    orders: Vec<super::Order>,
}

impl StandingOrder {
    pub fn new(account_id: u16, batch_index: u32, orders: Vec<super::Order>) -> StandingOrder {
        StandingOrder { account_id, batch_index, orders }
    }
    pub fn get_orders(&self) -> &Vec<super::Order> {
        &self.orders
    }
}

impl ConcatingHashable for Vec<StandingOrder> {
    fn concating_hash(&self, init_hash: H256) -> H256 {
       let mut hasher = Sha256::new();
        hasher.input(init_hash);
        for i in 0..models::NUM_RESERVED_ACCOUNTS {
            hasher.input(self
                .iter()
                .position(|o| o.account_id == i as u16) 
                .map(|k| self[k].get_orders())
                .map(|o| o.rolling_hash(0))
                .unwrap_or(H256::zero()));
        }
        let result = hasher.result();
        let b: Vec<u8> = result.to_vec();
        H256::from(b.as_slice())
    }
}

impl From<mongodb::ordered::OrderedDocument> for StandingOrder {
    fn from(document: mongodb::ordered::OrderedDocument) -> Self {
        let account_id = document.get_i32("_id").unwrap() as u16;
        let batch_index = document.get_i32("batchIndex").unwrap() as u32;
        StandingOrder {
            account_id,
            batch_index,
            orders: document
                .get_array("orders")
                .unwrap()
                .iter()
                .map(|raw_order| raw_order.as_document().unwrap())
                .map(|order_doc| super::Order {
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