use crate::contracts::stablex_contract::{FilteredOrderPage, StableXContract};
use crate::models::{AccountState, Order};
use crate::util::FutureWaitExt as _;

use super::auction_data_reader::IndexedAuctionDataReader;
use super::filtered_orderbook::OrderbookFilter;
use super::StableXOrderBookReading;

use anyhow::Result;
use ethcontract::{Address, U256};
use std::sync::Arc;

pub struct OnchainFilteredOrderBookReader {
    contract: Arc<dyn StableXContract + Send + Sync>,
    page_size: u16,
    filter: Vec<u16>,
}

impl OnchainFilteredOrderBookReader {
    pub fn new(
        contract: Arc<dyn StableXContract + Send + Sync>,
        page_size: u16,
        filter: &OrderbookFilter,
    ) -> Self {
        Self {
            contract,
            page_size,
            filter: filter
                .whitelist()
                .map(|set| set.iter().cloned().collect())
                .unwrap_or_else(|| vec![]),
        }
    }
}

impl StableXOrderBookReading for OnchainFilteredOrderBookReader {
    fn get_auction_data(&self, batch_id_to_solve: U256) -> Result<(AccountState, Vec<Order>)> {
        let last_block = self
            .contract
            .get_last_block_for_batch(batch_id_to_solve.as_u32())
            .wait()?;
        let mut reader = IndexedAuctionDataReader::new(batch_id_to_solve);
        let mut auction_data = FilteredOrderPage {
            indexed_elements: vec![],
            has_next_page: true,
            next_page_user: Address::zero(),
            next_page_user_offset: 0,
        };
        while auction_data.has_next_page {
            auction_data = self
                .contract
                .get_filtered_auction_data_paginated(
                    batch_id_to_solve,
                    self.filter.clone(),
                    self.page_size,
                    auction_data.next_page_user,
                    auction_data.next_page_user_offset,
                    Some(last_block.into()),
                )
                .wait()?;
            reader.apply_page(&auction_data.indexed_elements);
        }
        Ok(reader.get_auction_data())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::contracts::stablex_contract::MockStableXContract;
    use futures::future::FutureExt as _;
    use mockall::Sequence;

    const FIRST_ORDER: &[u8] = &[
        // order 1
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, // user: 20 elements
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 4, // sellTokenBalance: 4, 32 elements
        1, 2, // buyToken: 256+2,
        1, 1, // sellToken: 256+1, 56
        0, 0, 0, 2, // validFrom: 2
        0, 0, 1, 5, // validUntil: 256+5 64
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 2, // priceNumerator: 258
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 3, // priceDenominator: 259
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 1, // remainingAmount: 2**8 + 1 = 257
        0, 0, // order index
    ];
    const SECOND_ORDER: &[u8] = &[
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, // user:
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 5, // sellTokenBalance: 5
        1, 1, // buyToken: 256+1
        1, 2, // sellToken: 256+2
        0, 0, 0, 2, // validFrom: 2
        0, 0, 1, 5, // validUntil: 256+5
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 2, // priceNumerator: 258;
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 3, // priceDenominator: 259
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 0, // remainingAmount: 2**8 = 256
        0, 1, // order index
    ];

    #[test]
    fn test_no_data() {
        let mut contract = MockStableXContract::new();

        contract
            .expect_get_last_block_for_batch()
            .returning(|_| async { Ok(42) }.boxed());
        contract
            .expect_get_filtered_auction_data_paginated()
            .times(1)
            .returning(|_, _, _, _, _, _| {
                async {
                    Ok(FilteredOrderPage {
                        indexed_elements: vec![],
                        has_next_page: false,
                        next_page_user: Address::zero(),
                        next_page_user_offset: 1,
                    })
                }
                .boxed()
            });

        let reader = OnchainFilteredOrderBookReader::new(
            Arc::new(contract),
            10,
            &OrderbookFilter::default(),
        );
        assert_eq!(
            reader.get_auction_data(U256::from(42)).unwrap(),
            (AccountState::default(), vec![])
        )
    }

    #[test]
    fn test_single_page() {
        let mut contract = MockStableXContract::new();

        contract
            .expect_get_last_block_for_batch()
            .returning(|_| async { Ok(42) }.boxed());
        contract
            .expect_get_filtered_auction_data_paginated()
            .times(1)
            .returning(|_, _, _, _, _, _| {
                async {
                    Ok(FilteredOrderPage {
                        indexed_elements: FIRST_ORDER.to_vec(),
                        has_next_page: false,
                        next_page_user: Address::from_low_u64_be(1),
                        next_page_user_offset: 0,
                    })
                }
                .boxed()
            });

        let reader = OnchainFilteredOrderBookReader::new(
            Arc::new(contract),
            10,
            &OrderbookFilter::default(),
        );

        let mut state = AccountState::default();
        state.increase_balance(Address::from_low_u64_be(1), 257, 4);
        let order = Order {
            id: 0,
            account_id: Address::from_low_u64_be(1),
            buy_token: 258,
            sell_token: 257,
            buy_amount: 257,
            sell_amount: 257,
        };

        assert_eq!(
            reader.get_auction_data(U256::from(42)).unwrap(),
            (state, vec![order])
        )
    }

    #[test]
    fn test_two_pages() {
        let mut contract = MockStableXContract::new();
        let mut seq = Sequence::new();

        contract
            .expect_get_last_block_for_batch()
            .returning(|_| async { Ok(42) }.boxed());
        contract
            .expect_get_filtered_auction_data_paginated()
            .times(1)
            .in_sequence(&mut seq)
            .returning(|_, _, _, _, _, _| {
                async {
                    Ok(FilteredOrderPage {
                        indexed_elements: FIRST_ORDER.to_vec(),
                        has_next_page: true,
                        next_page_user: Address::from_low_u64_be(1),
                        next_page_user_offset: 0,
                    })
                }
                .boxed()
            });

        contract
            .expect_get_filtered_auction_data_paginated()
            .times(1)
            .in_sequence(&mut seq)
            .returning(|_, _, _, _, _, _| {
                async {
                    Ok(FilteredOrderPage {
                        indexed_elements: SECOND_ORDER.to_vec(),
                        has_next_page: false,
                        next_page_user: Address::from_low_u64_be(1),
                        next_page_user_offset: 1,
                    })
                }
                .boxed()
            });

        let reader = OnchainFilteredOrderBookReader::new(
            Arc::new(contract),
            10,
            &OrderbookFilter::default(),
        );

        let mut state = AccountState::default();
        state.increase_balance(Address::from_low_u64_be(1), 257, 4);
        state.increase_balance(Address::from_low_u64_be(1), 258, 5);
        let orders = vec![
            Order {
                id: 0,
                account_id: Address::from_low_u64_be(1),
                buy_token: 258,
                sell_token: 257,
                buy_amount: 257,
                sell_amount: 257,
            },
            Order {
                id: 1,
                account_id: Address::from_low_u64_be(1),
                buy_token: 257,
                sell_token: 258,
                buy_amount: 256,
                sell_amount: 256,
            },
        ];

        assert_eq!(
            reader.get_auction_data(U256::from(42)).unwrap(),
            (state, orders)
        )
    }
}
