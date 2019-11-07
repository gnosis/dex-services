use byteorder::WriteBytesExt;
use graph::data::store::Entity;
use serde_derive::Deserialize;
use std::convert::TryInto;
use std::sync::Arc;
use web3::types::{Log, H160, H256, U256};

use crate::models::util::*;
use crate::models::{iter_hash, RollingHashable, Serializable};
use crate::util::*;

#[derive(Debug, Clone, Deserialize, Eq, Ord, PartialEq, PartialOrd)]
#[serde(rename_all = "camelCase")]
pub struct BatchInformation {
    pub slot: U256,
    pub slot_index: u16,
}

#[derive(Debug, Clone, Default, Deserialize, Eq, Ord, PartialEq, PartialOrd)]
#[serde(rename_all = "camelCase")]
pub struct Order {
    pub batch_information: Option<BatchInformation>,
    pub account_id: H160,
    pub buy_token: u16,
    pub sell_token: u16,
    pub buy_amount: u128,
    pub sell_amount: u128,
}

impl Order {
    pub fn from_encoded_order(account_id: H160, bytes: &[u8; 26]) -> Self {
        let buy_token = u16::from(u8::from_le_bytes([bytes[25]])); // 1 byte
        let sell_token = u16::from(u8::from_le_bytes([bytes[24]])); // 1 byte
        let sell_amount = read_amount(
            &get_amount_from_slice(&bytes[12..24]), // 12 bytes
        );
        let buy_amount = read_amount(
            &get_amount_from_slice(&bytes[0..12]), // 12 bytes
        );

        Order {
            batch_information: None,
            account_id,
            buy_token,
            sell_token,
            buy_amount,
            sell_amount,
        }
    }

    /// Calculates the utility of an order based on a buy and sell price as well
    /// as the executed buy amount.
    ///
    /// # Returns
    ///
    /// Returns utility of an order or None when there was an error in the
    /// calculation such as overflow or divide by 0.
    pub fn utility(
        &self,
        buy_price: u128,
        exec_buy_amount: u128,
        exec_sell_amount: u128,
    ) -> Option<U256> {
        let buy_price = u128_to_u256(buy_price);
        let exec_buy_amount = u128_to_u256(exec_buy_amount);
        let exec_sell_amount = u128_to_u256(exec_sell_amount);
        let buy_amount = u128_to_u256(self.buy_amount);
        let sell_amount = u128_to_u256(self.sell_amount);

        // essential_utility = (exec_buy_amount - (exec_sell_amount * buy_amount)
        //   / sell_amount) * buy_price
        // essential_utility = (((exec_sell_amount * buy_amount) % sell_amount)
        //   * buy_price) / sell_amount
        // utility = essential_utility - essential_utility

        let essential_utility = exec_buy_amount
            .checked_sub((exec_sell_amount * buy_amount).checked_div(sell_amount)?)?
            * buy_price;
        let utility_error =
            (((exec_sell_amount * buy_amount) % sell_amount) * buy_price) / sell_amount;
        let utility = essential_utility.checked_sub(utility_error)?;

        Some(utility)
    }

    /// Calculates the disregarded utility of an order based on a buy and sell
    /// price as well as the executed sell amount.
    ///
    /// # Returns
    ///
    /// Returns disregarded utility of an order or None when there was an error
    /// in the calculation such as overflow or divide by 0.
    pub fn disregarded_utility(
        &self,
        buy_price: u128,
        sell_price: u128,
        exec_sell_amount: u128,
    ) -> Option<U256> {
        let buy_price = u128_to_u256(buy_price);
        let sell_price = u128_to_u256(sell_price);
        let exec_sell_amount = u128_to_u256(exec_sell_amount);
        let buy_amount = u128_to_u256(self.buy_amount);
        let sell_amount = u128_to_u256(self.sell_amount);

        // limit_term = sell_price * sell_amount - buy_amount * buy_price
        // leftover_sell_amount = sell_amount - exec_sell_amount
        // disregarded_utility = (limit_term * leftover_sell_amount) / sell_amount

        let limit_term = (sell_price * sell_amount).checked_sub(buy_amount * buy_price)?;
        let leftover_sell_amount = sell_amount.checked_sub(exec_sell_amount)?;
        let disregarded_utility = limit_term
            .checked_mul(leftover_sell_amount)?
            .checked_div(sell_amount)?;

        Some(disregarded_utility)
    }
}

impl Serializable for Order {
    fn bytes(&self) -> Vec<u8> {
        let mut wtr = vec![0; 4];
        wtr.extend(self.buy_amount.bytes());
        wtr.extend(self.sell_amount.bytes());
        wtr.write_u8(self.sell_token.try_into().unwrap()).unwrap();
        wtr.write_u8(self.buy_token.try_into().unwrap()).unwrap();
        // For now we only write the low 2 bytes, since for the purpose of hashing,
        // the account space is still 16 bits
        wtr.write_u8(self.account_id[18]).unwrap();
        wtr.write_u8(self.account_id[19]).unwrap();
        wtr
    }
}

impl Serializable for u128 {
    fn bytes(&self) -> Vec<u8> {
        self.to_be_bytes()[4..].to_vec()
    }
}

impl From<Arc<Log>> for Order {
    fn from(log: Arc<Log>) -> Self {
        let mut bytes: Vec<u8> = log.data.0.clone();
        Order {
            batch_information: Some(BatchInformation {
                slot: U256::pop_from_log_data(&mut bytes),
                slot_index: u16::pop_from_log_data(&mut bytes),
            }),
            account_id: H160::pop_from_log_data(&mut bytes),
            buy_token: u16::pop_from_log_data(&mut bytes),
            sell_token: u16::pop_from_log_data(&mut bytes),
            buy_amount: u128::pop_from_log_data(&mut bytes),
            sell_amount: u128::pop_from_log_data(&mut bytes),
        }
    }
}

impl From<Entity> for Order {
    fn from(entity: Entity) -> Self {
        let batch_information = entity
            .get("auctionId")
            .and(entity.get("slotIndex"))
            .and_then(|_| {
                Some(BatchInformation {
                    slot: U256::from_entity(&entity, "auctionId"),
                    slot_index: u16::from_entity(&entity, "slotIndex"),
                })
            });

        Order {
            batch_information,
            account_id: H160::from_entity(&entity, "accountId"),
            buy_token: u16::from_entity(&entity, "buyToken"),
            sell_token: u16::from_entity(&entity, "sellToken"),
            buy_amount: u128::from_entity(&entity, "buyAmount"),
            sell_amount: u128::from_entity(&entity, "sellAmount"),
        }
    }
}

impl Into<Entity> for Order {
    fn into(self) -> Entity {
        let mut entity = Entity::new();
        if let Some(_batch_info) = self.batch_information {
            entity.set("auctionId", _batch_info.slot.to_value());
            entity.set("slotIndex", _batch_info.slot_index.to_value());
        }
        entity.set("accountId", self.account_id.to_value());
        entity.set("buyToken", self.buy_token.to_value());
        entity.set("sellToken", self.sell_token.to_value());
        entity.set("buyAmount", self.buy_amount.to_value());
        entity.set("sellAmount", self.sell_amount.to_value());
        entity
    }
}

impl<T: Serializable> RollingHashable for Vec<T> {
    fn rolling_hash(&self, nonce: u32) -> H256 {
        self.iter()
            .fold(H256::from(u64::from(nonce)), |acc, w| iter_hash(w, &acc))
    }
}

pub mod test_util {
    use super::*;
    pub fn create_order_for_test() -> Order {
        Order {
            batch_information: Some(BatchInformation {
                slot: U256::zero(),
                slot_index: 0,
            }),
            account_id: H160::from(1),
            buy_token: 3,
            sell_token: 2,
            buy_amount: 5,
            sell_amount: 4,
        }
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use graph::bigdecimal::BigDecimal;
    use std::str::FromStr;
    use web3::types::{Bytes, H256};

    #[test]
    fn test_order_rolling_hash() {
        let order = Order {
            batch_information: Some(BatchInformation {
                slot: U256::zero(),
                slot_index: 0,
            }),
            account_id: H160::from(1),
            buy_token: 3,
            sell_token: 2,
            buy_amount: 5,
            sell_amount: 4,
        };

        assert_eq!(
            vec![order].rolling_hash(0),
            H256::from_str("8c253b4588a6d87b02b5f7d1424020b7b5f8c0397e464e087d2830a126d3b699")
                .unwrap()
        );
    }

    #[test]
    fn test_from_log() {
        let bytes: Vec<Vec<u8>> = vec![
            /* slot_bytes */
            vec![
                0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                0, 0, 0, 0,
            ],
            /* slot_index_bytes */
            vec![
                0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                0, 0, 0, 0,
            ],
            /* account_id_bytes */
            vec![
                0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                0, 0, 0, 1,
            ],
            /* buy_token_bytes */
            vec![
                0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                0, 0, 0, 3,
            ],
            /* sell_token_bytes */
            vec![
                0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                0, 0, 0, 2,
            ],
            /* buy_amount_bytes */
            vec![
                0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 13, 224,
                182, 179, 167, 100, 0, 0,
            ],
            /* sell_amount_bytes */
            vec![
                0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 13, 224,
                182, 179, 167, 100, 0, 0,
            ],
        ];

        let log = Arc::new(Log {
            address: 1.into(),
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
        });

        let expected_order = Order {
            batch_information: Some(BatchInformation {
                slot: U256::zero(),
                slot_index: 0,
            }),
            account_id: H160::from(1),
            buy_token: 3,
            sell_token: 2,
            buy_amount: (10 as u128).pow(18),
            sell_amount: (10 as u128).pow(18),
        };

        assert_eq!(expected_order, Order::from(log));
    }

    #[test]
    fn test_into_and_from_entity() {
        let order = create_order_for_test();
        let entity = create_entity_for_test();

        assert_eq!(entity, order.clone().into());
        assert_eq!(order, Order::from(entity));
    }

    #[test]
    fn test_into_entity_no_slot() {
        let mut expected_entity: Entity = create_entity_for_test();
        expected_entity.remove("slotIndex");
        expected_entity.remove("auctionId");

        let mut order: Order = create_order_for_test();
        order.batch_information = None;
        let actual_entity: Entity = order.into();

        assert_eq!(expected_entity, actual_entity);
    }

    #[test]
    fn test_from_entity_no_slot() {
        let mut expected_order = create_order_for_test();
        expected_order.batch_information = None;
        let entity = create_entity_for_test();

        let mut actual_entity = entity.clone();
        actual_entity.remove("slotIndex");
        assert_eq!(
            expected_order,
            Order::from(actual_entity),
            "No batch info if there's no slot index"
        );

        let mut actual_entity = entity.clone();
        actual_entity.remove("auctionId");
        assert_eq!(
            expected_order,
            Order::from(actual_entity),
            "No batch info if there's no auctionId"
        );

        let mut actual_entity = entity.clone();
        actual_entity.remove("slotIndex");
        actual_entity.remove("auctionId");
        assert_eq!(
            expected_order,
            Order::from(actual_entity),
            "No batch info if there's no slot index and auctionId"
        );
    }

    #[test]
    fn test_utility_calculation_for_small_order() {
        let order = Order {
            buy_amount: 10,
            sell_amount: 100,
            ..create_order_for_test()
        };
        let buy_price = 9;
        let exec_buy_amount = 10;
        let exec_sell_amount = 90;

        assert_eq!(
            order.utility(buy_price, exec_buy_amount, exec_sell_amount),
            // u = ((xb * os - xs * ob) * bp) / os
            //   = ((10 * 100 - 90 * 10) * 9) / 100
            //   = 100
            Some(9.into())
        );
    }

    #[test]
    fn test_utility_calculation_for_large_order() {
        let order = create_order_for_test();
        let buy_price = 20 * 10u128.pow(18);
        let exec_buy_amount = 2 * 10u128.pow(18);
        let exec_sell_amount = 10u128.pow(18);

        assert_eq!(
            order.utility(buy_price, exec_buy_amount, exec_sell_amount),
            // u = ((2e18 * 2e18 - 1e18 * 1e18) * 20e18) / 2e18
            //   = 3e37
            Some(U256::from(3) * U256::from(10).pow(37.into()))
        );
    }

    #[test]
    fn test_utility_calculation_for_exact_match() {
        let order = create_order_for_test();
        assert_eq!(
            order.utility(1, order.buy_amount, order.sell_amount),
            Some(U256::zero())
        );
    }

    #[test]
    fn test_utility_overflow() {
        // calculation errors happen when `exec_sell_amount * buy_amount >
        // exec_buy_amount * sell_amount` or if sell_amount is 0

        let order = create_order_for_test();
        let exec_sell_amount = order.sell_amount * 2;
        assert_eq!(order.utility(1, order.buy_amount, exec_sell_amount), None);

        let order = Order {
            sell_amount: 0,
            ..create_order_for_test()
        };
        assert_eq!(order.utility(1, order.buy_amount, order.sell_amount), None);
    }

    #[test]
    fn test_disregarded_utility_calculation_for_small_order() {
        let order = Order {
            buy_amount: 10,
            sell_amount: 100,
            ..create_order_for_test()
        };
        let buy_price = 9;
        let sell_price = 1;
        let exec_sell_amount = 90;

        assert_eq!(
            order.disregarded_utility(buy_price, sell_price, exec_sell_amount),
            // du = ((ps * os - ob * pb) * (os - xs)) / os
            //    = ((1 * 100 - 10 * 9) * (100 - 90)) / 100
            //    = 1
            Some(1.into())
        );
    }

    #[test]
    fn test_disregarded_utility_calculation_for_large_order() {
        let order = create_order_for_test();
        let buy_price = 20 * 10u128.pow(18);
        let sell_price = 40 * 10u128.pow(18);
        let exec_sell_amount = 10u128.pow(18);

        assert_eq!(
            order.disregarded_utility(buy_price, sell_price, exec_sell_amount),
            // du = ((ps * os - ob * pb) * (os - xs)) / os
            //    = ((40e18 * 2e18 - 1e18 * 20e18) * (2e18 - 1e18)) / 2e18
            //    = 3e37
            Some(U256::from(3) * U256::from(10).pow(37.into()))
        );
    }

    #[test]
    fn test_disregarded_utility_calculation_for_exact_match() {
        let order = create_order_for_test();
        assert_eq!(
            order.disregarded_utility(2, 1, order.sell_amount),
            Some(U256::zero())
        );
    }

    #[test]
    fn test_disregarded_utility_overflow() {
        // disregarded utility calculation error happen on `exec_sell_amount >
        // sell_amount`, `sell_price * sell_amount < buy_amount * buy_price`,
        // `sell_amount = 0`, and `limit_term * leftover_sell_amount` overflow

        let order = create_order_for_test();
        let exec_sell_amount = order.sell_amount + 1;
        assert_eq!(order.disregarded_utility(0, 1, exec_sell_amount), None);

        assert_eq!(order.disregarded_utility(10, 1, order.sell_amount), None);

        let order = Order {
            sell_amount: 0,
            ..create_order_for_test()
        };
        assert_eq!(order.disregarded_utility(0, 0, order.sell_amount), None);

        let order = Order {
            buy_amount: 1,
            sell_amount: u128::max_value(),
            ..create_order_for_test()
        };
        let buy_price = u128::max_value();
        let sell_price = u128::max_value();
        let exec_sell_amount = 1;
        assert_eq!(
            order.disregarded_utility(buy_price, sell_price, exec_sell_amount),
            None
        );
    }

    fn create_order_for_test() -> Order {
        Order {
            batch_information: Some(BatchInformation {
                slot: U256::zero(),
                slot_index: 0,
            }),
            account_id: H160::from(1),
            buy_token: 1,
            sell_token: 2,
            buy_amount: 10u128.pow(18),
            sell_amount: 2 * 10u128.pow(18),
        }
    }

    fn create_entity_for_test() -> Entity {
        let mut entity = Entity::new();
        entity.set("auctionId", BigDecimal::from(0));
        entity.set("slotIndex", 0);
        entity.set("accountId", "0000000000000000000000000000000000000001");
        entity.set("buyToken", 1);
        entity.set("sellToken", 2);
        entity.set("buyAmount", BigDecimal::from(10u64.pow(18)));
        entity.set("sellAmount", BigDecimal::from(2 * 10u64.pow(18)));

        entity
    }
}
