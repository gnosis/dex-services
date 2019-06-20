use serde_derive::{Deserialize};

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