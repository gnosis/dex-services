use crate::contracts::stablex_auction_element::{StableXAuctionElement, AUCTION_ELEMENT_WIDTH};
use crate::models::{AccountState, Order};
use std::collections::HashMap;
use web3::types::{H160, U256};

/// Handles reading of auction data that has been encoded with the smart
/// contract's `encodeAuctionElement` function.
pub struct BatchedAuctionDataReader {
    /// The account state resulting from unfiltered handled orders.
    account_state: AccountState,
    /// All unfiltered orders in the order they were received.
    orders: Vec<Order>,
    /// Used when reading data from the smart contract in batches.
    pagination: Pagination,
    /// The index of batch whose orders we are filtering for.
    index: U256,
    /// The total number of orders per user.
    user_order_counts: HashMap<H160, usize>,
}

/// Data for the next call to the smart contract's `getEncodedUsersPaginated`
/// function (which should be named `getEncodedOrdersPaginated`).
pub struct Pagination {
    /// The user of the last received order or H160::zero when no order has been
    /// received.
    pub previous_page_user: H160,
    /// The number of received orders for `previous_page_user`.
    pub previous_page_user_offset: usize,
}

impl BatchedAuctionDataReader {
    /// Create a new BatchedAuctionDataReader.
    pub fn new(index: U256) -> BatchedAuctionDataReader {
        BatchedAuctionDataReader {
            account_state: AccountState::default(),
            orders: Vec::new(),
            pagination: Pagination {
                previous_page_user: H160::zero(),
                previous_page_user_offset: 0,
            },
            index,
            user_order_counts: HashMap::new(),
        }
    }

    /// The pagination data used when reading data from the smart contract in
    /// batches.
    pub fn pagination(&self) -> &Pagination {
        &self.pagination
    }

    /// Signal that no more data will be read and return the result consuming
    /// self.
    pub fn get_auction_data(self) -> (AccountState, Vec<Order>) {
        (self.account_state, self.orders)
    }

    /// Applies one batch of data.
    ///
    /// A batch can come from `getEncodedUsersPaginated` or `getEncodedOrders`.
    /// In the latter case there is only one batch.
    ///
    /// Returns the number of orders in the data.
    ///
    /// Panics if length of `packed_auction_bytes` is not a multiple of
    /// `AUCTION_ELEMENT_WIDTH`.
    pub fn apply_batch(&mut self, packed_auction_bytes: &[u8]) -> usize {
        let previous_order_count = self.orders.len();
        self.apply_auction_data(&packed_auction_bytes);
        let number_of_added_orders = self.orders.len() - previous_order_count;
        if number_of_added_orders == 0 {
            return 0;
        }
        let last_order_user = self.orders.last().expect("there are no orders").account_id;
        self.pagination.previous_page_user = last_order_user;
        self.pagination.previous_page_user_offset = *self
            .user_order_counts
            .get(&last_order_user)
            .expect("user has order but no order count");
        number_of_added_orders
    }

    fn apply_auction_data(&mut self, packed_auction_bytes: &[u8]) {
        let auction_elements = self.parse_auction_elements(packed_auction_bytes);
        self.apply_auction_elements_to_account_state(auction_elements.iter());
        self.orders.extend(
            auction_elements
                .into_iter()
                .map(|auction_element| auction_element.order),
        );
    }

    fn apply_auction_elements_to_account_state<'a, Iter>(&mut self, auction_elements: Iter)
    where
        Iter: Iterator<Item = &'a StableXAuctionElement>,
    {
        for element in auction_elements.into_iter() {
            self.account_state.modify_balance(
                element.order.account_id,
                element.order.sell_token,
                |x| {
                    if *x == 0 {
                        *x = element.sell_token_balance
                    } else {
                        assert_eq!(
                            *x,
                            element.sell_token_balance,
                            "got order which sets user {}'s sell token {} \
                            balance to {} but sell_token_balance has already \
                            been set to {}",
                            element.order.account_id,
                            element.order.sell_token,
                            element.sell_token_balance,
                            *x
                        );
                    }
                },
            );
        }
    }

    fn parse_auction_elements(
        &mut self,
        packed_auction_bytes: &[u8],
    ) -> Vec<StableXAuctionElement> {
        assert_eq!(
            packed_auction_bytes.len() % AUCTION_ELEMENT_WIDTH,
            0,
            "Each auction should be packed in {} bytes",
            AUCTION_ELEMENT_WIDTH
        );

        // Workaround for borrow checker that would complain that the map
        // closure borrows self as mutable while at the same time the filter
        // closure borrows self as immutable because of using `self.index`.
        let index = self.index;
        packed_auction_bytes
            .chunks(AUCTION_ELEMENT_WIDTH)
            .map(|chunk| {
                let mut chunk_array = [0; AUCTION_ELEMENT_WIDTH];
                chunk_array.copy_from_slice(chunk);
                let mut result = StableXAuctionElement::from_bytes(&chunk_array);
                let order_counter = self
                    .user_order_counts
                    .entry(result.order.account_id)
                    .or_insert(0);
                result.order.id = *order_counter as u16;
                *order_counter += 1;
                result
            })
            .filter(|x| x.in_auction(index) && x.order.sell_amount > 0)
            .collect()
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use lazy_static::lazy_static;

    const ORDER_1_BYTES: &[u8] = &[
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
    ];
    const ORDER_2_BYTES: &[u8] = &[
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
    ];
    const ORDER_3_BYTES: &[u8] = &[
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 2, // user:
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 6, // sellTokenBalance: 6
        1, 2, // buyToken: 256+2
        1, 1, // sellToken: 256+1
        0, 0, 0, 2, // validFrom: 2
        0, 0, 1, 5, // validUntil: 256+5
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 2, // priceNumerator: 258;
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 3, // priceDenominator: 259
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 0, // remainingAmount: 2**8 = 256
    ];

    lazy_static! {
        static ref ORDER_1: Order = Order {
            id: 0,
            account_id: H160::from_low_u64_be(1),
            sell_token: 257,
            buy_token: 258,
            sell_amount: 257,
            buy_amount: 257,
        };
        static ref ORDER_2: Order = Order {
            id: 1,
            account_id: H160::from_low_u64_be(1),
            sell_token: 258,
            buy_token: 257,
            sell_amount: 256,
            buy_amount: 256,
        };
    }

    #[test]
    fn batched_auction_data_reader_empty() {
        let mut reader = BatchedAuctionDataReader::new(U256::from(3));
        assert_eq!(reader.apply_batch(&[]), 0);
    }

    #[test]
    fn batched_auction_data_reader_single_batch() {
        let mut bytes = Vec::new();
        bytes.extend(ORDER_1_BYTES);
        bytes.extend(ORDER_2_BYTES);
        let mut reader = BatchedAuctionDataReader::new(U256::from(3));
        assert_eq!(reader.apply_batch(&bytes), 2);

        let mut account_state = AccountState::default();
        account_state.modify_balance(H160::from_low_u64_be(1), 257, |x| *x = 4);
        account_state.modify_balance(H160::from_low_u64_be(1), 258, |x| *x = 5);

        assert_eq!(reader.account_state, account_state);
        assert_eq!(reader.orders, [ORDER_1.clone(), ORDER_2.clone()]);
        assert_eq!(reader.pagination.previous_page_user, ORDER_1.account_id);
        assert_eq!(reader.pagination.previous_page_user_offset, 2);
    }

    #[test]
    fn batched_auction_data_reader_multiple_batches() {
        let mut account_state = AccountState::default();
        let mut reader = BatchedAuctionDataReader::new(U256::from(3));
        let mut bytes = Vec::new();

        bytes.extend(ORDER_1_BYTES);
        assert_eq!(reader.apply_batch(&bytes), 1);
        account_state.modify_balance(H160::from_low_u64_be(1), 257, |x| *x = 4);
        assert_eq!(reader.account_state, account_state);
        assert_eq!(reader.orders, [ORDER_1.clone()]);
        assert_eq!(reader.pagination.previous_page_user, ORDER_1.account_id);
        assert_eq!(reader.pagination.previous_page_user_offset, 1);

        bytes.clear();
        bytes.extend(ORDER_2_BYTES);
        assert_eq!(reader.apply_batch(&bytes), 1);
        account_state.modify_balance(H160::from_low_u64_be(1), 258, |x| *x = 5);
        assert_eq!(reader.account_state, account_state);
        assert_eq!(reader.orders, [ORDER_1.clone(), ORDER_2.clone()]);
        assert_eq!(reader.pagination.previous_page_user, ORDER_1.account_id);
        assert_eq!(reader.pagination.previous_page_user_offset, 2);

        bytes.clear();
        bytes.extend(ORDER_3_BYTES);
        assert_eq!(reader.apply_batch(&bytes), 1);
        account_state.modify_balance(H160::from_low_u64_be(2), 257, |x| *x = 6);
        assert_eq!(reader.account_state, account_state);
        assert_eq!(
            reader.pagination.previous_page_user,
            H160::from_low_u64_be(2)
        );
        assert_eq!(reader.pagination.previous_page_user_offset, 1);
    }

    #[test]
    fn batched_auction_data_reader_multiple_batches_different_users() {
        let mut account_state = AccountState::default();
        let mut reader = BatchedAuctionDataReader::new(U256::from(3));
        let mut bytes = Vec::new();

        bytes.extend(ORDER_1_BYTES);
        assert_eq!(reader.apply_batch(&bytes), 1);
        account_state.modify_balance(H160::from_low_u64_be(1), 257, |x| *x = 4);
        assert_eq!(reader.account_state, account_state);
        assert_eq!(reader.orders, [ORDER_1.clone()]);
        assert_eq!(reader.pagination.previous_page_user, ORDER_1.account_id);
        assert_eq!(reader.pagination.previous_page_user_offset, 1);

        bytes.clear();
        bytes.extend(ORDER_2_BYTES);
        bytes.extend(ORDER_3_BYTES);
        assert_eq!(reader.apply_batch(&bytes), 2);
        account_state.modify_balance(H160::from_low_u64_be(1), 258, |x| *x = 5);
        account_state.modify_balance(H160::from_low_u64_be(2), 257, |x| *x = 6);
        assert_eq!(reader.account_state, account_state);
        assert_eq!(
            reader.pagination.previous_page_user,
            H160::from_low_u64_be(2)
        );
        assert_eq!(reader.pagination.previous_page_user_offset, 1);
    }

    #[test]
    #[should_panic]
    fn batched_auction_data_reader_panics() {
        let mut reader = BatchedAuctionDataReader::new(U256::from(3));
        let mut bytes = Vec::new();
        bytes.extend(ORDER_1_BYTES);
        assert_eq!(reader.apply_batch(&bytes), 1);
        bytes[51] += 1;
        // Incremented sell_token_balance which should cause a panic because it
        // does not match the previous balance.
        reader.apply_batch(&bytes);
    }
}
