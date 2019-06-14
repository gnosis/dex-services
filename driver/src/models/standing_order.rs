use serde_derive::{Deserialize};

#[derive(Clone, Deserialize, Eq, Ord, PartialEq, PartialOrd)]
#[serde(rename_all = "camelCase")]
pub struct StandingOrder {
    pub account_id: u16,
    orders: Vec<super::Order>,
}

impl From<mongodb::ordered::OrderedDocument> for StandingOrder {
    fn from(document: mongodb::ordered::OrderedDocument) -> Self {
        let account_id = document.get_i32("_id").unwrap() as u16;
        StandingOrder {
            account_id,
            orders: document
                .get_array("orders")
                .unwrap()
                .iter()
                .map(|o| o.as_document().unwrap())
                .map(|o| super::Order {
                        account_id,
                        buy_token: o.get_i32("buyToken").unwrap() as u8,
                        sell_token: o.get_i32("sellToken").unwrap() as u8,
                        buy_amount: o.get_str("buyAmount").unwrap().parse().unwrap(),
                        sell_amount: o.get_str("sellAmount").unwrap().parse().unwrap(),
                    }
                ).collect()
        }
    }
}