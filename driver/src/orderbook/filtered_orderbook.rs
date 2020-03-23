use super::*;

use crate::models::{AccountState, Order};
use anyhow::Error;
use ethcontract::Address;
use serde::Deserialize;
use std::collections::{HashMap, HashSet};
use std::str::FromStr;

/// Data structure to specify what type of orders to blacklist
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Eq)]
pub struct BlacklistOrderbookFilter {
    /// The token ids that should be filtered/
    #[serde(default)]
    tokens: HashSet<u16>,

    /// User addresses mapped to which of their orders to filter
    #[serde(default)]
    users: HashMap<Address, UserOrderFilter>,
}

/// Data structure to specify what type of orders to whitelist
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Eq)]
pub struct WhitelistOrderbookFilter {
    /// The token ids that should be allowed/
    #[serde(default)]
    tokens: HashSet<u16>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
enum UserOrderFilter {
    All,
    OrderIds(HashSet<u16>),
}

impl FromStr for BlacklistOrderbookFilter {
    type Err = Error;

    fn from_str(value: &str) -> Result<Self> {
        Ok(serde_json::from_str(value)?)
    }
}

impl FromStr for WhitelistOrderbookFilter {
    type Err = Error;

    fn from_str(value: &str) -> Result<Self> {
        Ok(serde_json::from_str(value)?)
    }
}

pub struct FilteredOrderbookReader<'a> {
    orderbook: &'a (dyn StableXOrderBookReading + Sync),
    whitelist_filter: WhitelistOrderbookFilter,
    blacklist_filter: BlacklistOrderbookFilter,
}

impl<'a> FilteredOrderbookReader<'a> {
    pub fn new(
        orderbook: &'a (dyn StableXOrderBookReading + Sync),
        whitelist_filter: WhitelistOrderbookFilter,
        blacklist_filter: BlacklistOrderbookFilter,
    ) -> Self {
        Self {
            orderbook,
            whitelist_filter,
            blacklist_filter,
        }
    }
}

impl<'a> StableXOrderBookReading for FilteredOrderbookReader<'a> {
    fn get_auction_data(&self, index: U256) -> Result<(AccountState, Vec<Order>)> {
        let (state, orders) = self.orderbook.get_auction_data(index)?;
        let mut filtered: Vec<Order> = orders
            .into_iter()
            .filter(|o| {
                let user_filter =
                    if let Some(user_filter) = self.blacklist_filter.users.get(&o.account_id) {
                        match user_filter {
                            UserOrderFilter::All => true,
                            UserOrderFilter::OrderIds(ids) => ids.contains(&o.id),
                        }
                    } else {
                        false
                    };
                !self.blacklist_filter.tokens.contains(&o.buy_token)
                    && !self.blacklist_filter.tokens.contains(&o.sell_token)
                    && !user_filter
            })
            .collect();
        if !self.whitelist_filter.tokens.is_empty() {
            filtered = filtered
                .into_iter()
                .filter(|o| {
                    self.whitelist_filter.tokens.contains(&o.buy_token)
                        && self.whitelist_filter.tokens.contains(&o.sell_token)
                })
                .collect();
        };
        Ok((state, filtered))
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::models::order::test_util::create_order_for_test;
    use std::str::FromStr;

    #[test]
    fn test_blacklist_filter_deserialization() {
        let json = r#"{
            "tokens": [1,2],
            "users": {
                "0x7b60655Ca240AC6c76dD29c13C45BEd969Ee6F0A": {"OrderIds": [0,1]},
                "0x7b60655Ca240AC6c76dD29c13C45BEd969Ee6F0B": "All"
            }
        }"#;
        let blacklist_filter = BlacklistOrderbookFilter {
            tokens: [1, 2].iter().copied().collect(),
            users: [
                (
                    Address::from_str("7b60655Ca240AC6c76dD29c13C45BEd969Ee6F0B").unwrap(),
                    UserOrderFilter::All,
                ),
                (
                    Address::from_str("7b60655Ca240AC6c76dD29c13C45BEd969Ee6F0A").unwrap(),
                    UserOrderFilter::OrderIds([0u16, 1u16].iter().copied().collect()),
                ),
            ]
            .iter()
            .cloned()
            .collect(),
        };
        assert_eq!(
            blacklist_filter,
            serde_json::from_str(json).expect("Failed to parse")
        );
    }

    #[test]
    fn test_whitelist_filter_deserialization() {
        let json = r#"{
            "tokens": [1,2]
        }"#;
        let whitelist_filter = WhitelistOrderbookFilter {
            tokens: [1, 2].iter().copied().collect(),
        };
        assert_eq!(
            whitelist_filter,
            serde_json::from_str(json).expect("Failed to parse")
        );
    }

    #[test]
    fn test_blacklist_orderbook_filter() {
        let mut bad_sell_token = create_order_for_test();
        bad_sell_token.sell_token = 4;
        let mut bad_buy_token = create_order_for_test();
        bad_buy_token.buy_token = 5;

        let mut bad_user = create_order_for_test();
        bad_user.account_id =
            Address::from_str("7b60655Ca240AC6c76dD29c13C45BEd969Ee6F0B").unwrap();

        let mixed_user = Address::from_str("7b60655Ca240AC6c76dD29c13C45BEd969Ee6F0A").unwrap();
        let mut mixed_user_good_order = create_order_for_test();
        mixed_user_good_order.account_id = mixed_user;
        mixed_user_good_order.id = 0;

        let mut mixed_user_bad_order = create_order_for_test();
        mixed_user_bad_order.account_id = mixed_user;
        mixed_user_bad_order.id = 1;

        let mut inner = MockStableXOrderBookReading::default();
        inner.expect_get_auction_data().return_once({
            let result = (
                AccountState::default(),
                vec![
                    bad_buy_token.clone(),
                    bad_sell_token.clone(),
                    mixed_user_bad_order,
                    mixed_user_good_order.clone(),
                ],
            );
            move |_| Ok(result)
        });

        let blacklist_filter = BlacklistOrderbookFilter {
            tokens: [bad_sell_token.sell_token, bad_buy_token.buy_token]
                .iter()
                .copied()
                .collect(),
            users: [
                (bad_user.account_id, UserOrderFilter::All),
                (
                    mixed_user,
                    UserOrderFilter::OrderIds([1].iter().copied().collect()),
                ),
            ]
            .iter()
            .cloned()
            .collect(),
        };
        let whitelist_filter = WhitelistOrderbookFilter {
            tokens: HashSet::new(),
        };

        let reader = FilteredOrderbookReader::new(&inner, whitelist_filter, blacklist_filter);

        let (_, filtered_orders) = reader.get_auction_data(U256::zero()).unwrap();
        assert_eq!(filtered_orders, vec![mixed_user_good_order]);
    }

    #[test]

    fn test_whitelist_orderbook_filter() {
        let mut bad_sell_token = create_order_for_test();
        bad_sell_token.sell_token = 4; // 4 will not be whitelisted
        let mut bad_buy_token = create_order_for_test();
        bad_buy_token.buy_token = 5; // 5 will not be whitelisted
        let good_order = create_order_for_test();

        let mut inner = MockStableXOrderBookReading::default();
        inner.expect_get_auction_data().return_once({
            let result = (
                AccountState::default(),
                vec![bad_buy_token, bad_sell_token, good_order.clone()],
            );
            move |_| Ok(result)
        });

        let blacklist_filter = BlacklistOrderbookFilter {
            tokens: HashSet::new(),
            users: HashMap::new(),
        };
        let whitelist_filter = WhitelistOrderbookFilter {
            tokens: [2, 3].iter().copied().collect(),
        };

        let reader = FilteredOrderbookReader::new(&inner, whitelist_filter, blacklist_filter);

        let (_, filtered_orders) = reader.get_auction_data(U256::zero()).unwrap();
        assert_eq!(filtered_orders, vec![good_order]);
    }

    #[test]
    fn test_empty_whitelist_orderbook_filter() {
        let good_order = create_order_for_test();

        let mut inner = MockStableXOrderBookReading::default();
        inner.expect_get_auction_data().return_once({
            let result = (AccountState::default(), vec![good_order.clone()]);
            move |_| Ok(result)
        });

        let blacklist_filter = BlacklistOrderbookFilter {
            tokens: HashSet::new(),
            users: HashMap::new(),
        };
        let whitelist_filter = WhitelistOrderbookFilter {
            tokens: HashSet::new(),
        };

        let reader = FilteredOrderbookReader::new(&inner, whitelist_filter, blacklist_filter);

        let (_, filtered_orders) = reader.get_auction_data(U256::zero()).unwrap();
        assert_eq!(filtered_orders, vec![good_order]);
    }
}
