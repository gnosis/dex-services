use super::*;

use crate::models::{AccountState, Order};
use anyhow::Error;
use ethcontract::Address;
use futures::future::{BoxFuture, FutureExt as _};
use serde::Deserialize;
use std::collections::{HashMap, HashSet};
use std::str::FromStr;

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
enum TokenFilter {
    Whitelist(HashSet<u16>),
    Blacklist(HashSet<u16>),
}

impl Default for TokenFilter {
    fn default() -> Self {
        TokenFilter::Blacklist(HashSet::new())
    }
}

/// Data structure to specify what type of orders to filter
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Eq)]
pub struct OrderbookFilter {
    /// The token ids that should be filtered/
    #[serde(default)]
    tokens: TokenFilter,

    /// User addresses mapped to which of their orders to filter
    #[serde(default)]
    users: HashMap<Address, UserOrderFilter>,
}

impl OrderbookFilter {
    pub fn whitelist(&self) -> Option<&HashSet<u16>> {
        match &self.tokens {
            TokenFilter::Whitelist(whitelist) => Some(whitelist),
            TokenFilter::Blacklist(_) => None,
        }
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
enum UserOrderFilter {
    All,
    OrderIds(HashSet<u16>),
}

impl FromStr for OrderbookFilter {
    type Err = Error;

    fn from_str(value: &str) -> Result<Self> {
        Ok(serde_json::from_str(value)?)
    }
}

pub struct FilteredOrderbookReader {
    orderbook: Box<dyn StableXOrderBookReading>,
    filter: OrderbookFilter,
}

impl FilteredOrderbookReader {
    pub fn new(orderbook: Box<dyn StableXOrderBookReading>, filter: OrderbookFilter) -> Self {
        Self { orderbook, filter }
    }
}

impl StableXOrderBookReading for FilteredOrderbookReader {
    fn get_auction_data<'b>(
        &'b self,
        batch_id_to_solve: u32,
    ) -> BoxFuture<'b, Result<(AccountState, Vec<Order>)>> {
        async move {
            let (state, orders) = self.orderbook.get_auction_data(batch_id_to_solve).await?;
            let token_filtered_orders: Vec<Order> = match &self.filter.tokens {
                TokenFilter::Whitelist(token_list) => orders
                    .into_iter()
                    .filter(|o| {
                        token_list.contains(&o.buy_token) && token_list.contains(&o.sell_token)
                    })
                    .collect(),
                TokenFilter::Blacklist(token_list) => orders
                    .into_iter()
                    .filter(|o| {
                        !token_list.contains(&o.buy_token) && !token_list.contains(&o.sell_token)
                    })
                    .collect(),
            };
            let user_filtered_orders = token_filtered_orders.into_iter().filter(|o| {
                if let Some(user_filter) = self.filter.users.get(&o.account_id) {
                    match user_filter {
                        UserOrderFilter::All => false,
                        UserOrderFilter::OrderIds(ids) => !ids.contains(&o.id),
                    }
                } else {
                    true
                }
            });
            Ok(util::normalize_auction_data(state, user_filtered_orders))
        }
        .boxed()
    }
    fn initialize<'b>(&'b self) -> BoxFuture<'b, Result<()>> {
        self.orderbook.initialize()
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
            "tokens": { "Blacklist": [1,2] },
            "users": {
                "0x7b60655Ca240AC6c76dD29c13C45BEd969Ee6F0A": {"OrderIds": [0,1]},
                "0x7b60655Ca240AC6c76dD29c13C45BEd969Ee6F0B": "All"
            }
        }"#;
        let blacklist_filter = OrderbookFilter {
            tokens: TokenFilter::Blacklist([1, 2].iter().copied().collect()),
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
            "tokens": { "Whitelist": [1,2] }
        }"#;
        let whitelist_filter = OrderbookFilter {
            tokens: TokenFilter::Whitelist([1, 2].iter().copied().collect()),
            users: HashMap::new(),
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
            move |_| async { Ok(result) }.boxed()
        });

        let filter = OrderbookFilter {
            tokens: TokenFilter::Blacklist(
                [bad_sell_token.sell_token, bad_buy_token.buy_token]
                    .iter()
                    .copied()
                    .collect(),
            ),
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

        let reader = FilteredOrderbookReader::new(Box::new(inner), filter);

        let (_, filtered_orders) = reader.get_auction_data(0).now_or_never().unwrap().unwrap();
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
            move |_| async { Ok(result) }.boxed()
        });

        let filter = OrderbookFilter {
            tokens: TokenFilter::Whitelist([2, 3].iter().copied().collect()),
            users: HashMap::new(),
        };

        let reader = FilteredOrderbookReader::new(Box::new(inner), filter);

        let (_, filtered_orders) = reader.get_auction_data(0).now_or_never().unwrap().unwrap();
        assert_eq!(filtered_orders, vec![good_order]);
    }

    #[test]
    fn test_filters_balances_for_which_there_are_no_sell_orders() {
        let mut state = AccountState::default();
        state.increase_balance(Address::zero(), 0, 10000);
        let mut inner = MockStableXOrderBookReading::default();
        inner
            .expect_get_auction_data()
            .return_once(move |_| async { Ok((state, vec![])) }.boxed());

        let filter = OrderbookFilter {
            tokens: TokenFilter::default(),
            users: HashMap::new(),
        };

        let reader = FilteredOrderbookReader::new(Box::new(inner), filter);

        let (state, filtered_orders) = reader.get_auction_data(0).now_or_never().unwrap().unwrap();
        assert_eq!(filtered_orders, vec![]);
        assert_eq!(state, AccountState::default());
    }
}
