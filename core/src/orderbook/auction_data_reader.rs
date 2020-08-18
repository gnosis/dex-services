use crate::contracts::stablex_auction_element::{
    StableXAuctionElement, INDEXED_AUCTION_ELEMENT_WIDTH,
};
use crate::models::{AccountState, Order};

/// Handles reading of auction data that has been encoded with the smart
/// contract's `encodeAuctionElement` function.
pub struct IndexedAuctionDataReader {
    /// The account state resulting from unfiltered handled orders.
    account_state: AccountState,
    /// All unfiltered orders in the order they were received.
    orders: Vec<Order>,
    /// Used when reading data from the smart contract in batches.
    index: u32,
}

impl IndexedAuctionDataReader {
    pub fn new(index: u32) -> Self {
        Self {
            account_state: AccountState::default(),
            orders: Vec::new(),
            index,
        }
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
    /// Panics if length of `packed_auction_bytes` is not a multiple of
    /// `AUCTION_ELEMENT_WIDTH`.
    pub fn apply_page(&mut self, packed_auction_bytes: &[u8]) {
        let auction_elements = parse_indexed_auction_elements(packed_auction_bytes);
        self.apply_auction_data(auction_elements);
    }

    fn apply_auction_data(&mut self, auction_elements: Vec<StableXAuctionElement>) {
        // Workaround for borrow checker that would complain that `extend`
        // borrows self as mutable while at the same time the filter
        // closure borrows self as immutable because of using `self.index`.
        let index = self.index;
        let auction_elements = auction_elements
            .iter()
            .filter(|x| x.in_auction(index) && x.order.remaining_sell_amount > 0);
        self.apply_auction_elements_to_account_state(auction_elements.clone());
        self.orders
            .extend(auction_elements.map(|auction_element| auction_element.order.clone()));
    }

    fn apply_auction_elements_to_account_state<'a, Iter>(&mut self, auction_elements: Iter)
    where
        Iter: Iterator<Item = &'a StableXAuctionElement>,
    {
        for element in auction_elements.into_iter() {
            match self.account_state.0.insert(
                (element.order.account_id, element.order.sell_token),
                element.sell_token_balance,
            ) {
                Some(old_balance) if old_balance != element.sell_token_balance => log::warn!(
                    "got order which sets user {}'s sell token {} \
                     balance to {} but sell_token_balance has already \
                     been set to {}",
                    element.order.account_id,
                    element.order.sell_token,
                    element.sell_token_balance,
                    old_balance,
                ),
                _ => (),
            }
        }
    }
}

fn parse_indexed_auction_elements(indexed_auction_bytes: &[u8]) -> Vec<StableXAuctionElement> {
    assert_eq!(
        indexed_auction_bytes.len() % INDEXED_AUCTION_ELEMENT_WIDTH,
        0,
        "Each auction should be packed in {} bytes",
        INDEXED_AUCTION_ELEMENT_WIDTH
    );

    indexed_auction_bytes
        .chunks(INDEXED_AUCTION_ELEMENT_WIDTH)
        .map(auction_element_from_indexed_slice)
        .collect()
}

fn auction_element_from_indexed_slice(chunk: &[u8]) -> StableXAuctionElement {
    let mut chunk_array = [0u8; INDEXED_AUCTION_ELEMENT_WIDTH];
    chunk_array.copy_from_slice(chunk);
    StableXAuctionElement::from_indexed_bytes(&chunk_array)
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use ethcontract::Address;
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
        0, 0, // orderId: 0
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
        0, 1, // orderId: 1
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
        0, 0, // orderId: 0
    ];

    lazy_static! {
        static ref ORDER_1: Order = Order {
            id: 0,
            account_id: Address::from_low_u64_be(1),
            sell_token: 257,
            buy_token: 258,
            denominator: 259,
            numerator: 258,
            remaining_sell_amount: 257,
            valid_from: 2,
            valid_until: 261,
        };
        static ref ORDER_2: Order = Order {
            id: 1,
            account_id: Address::from_low_u64_be(1),
            sell_token: 258,
            buy_token: 257,
            denominator: 259,
            numerator: 258,
            remaining_sell_amount: 256,
            valid_from: 2,
            valid_until: 261,
        };
        static ref ORDER_3: Order = Order {
            id: 0,
            account_id: Address::from_low_u64_be(2),
            sell_token: 258,
            buy_token: 257,
            denominator: 259,
            numerator: 258,
            remaining_sell_amount: 256,
            valid_from: 2,
            valid_until: 261,
        };
    }

    #[test]
    fn auction_data_reader_empty() {
        let mut reader = IndexedAuctionDataReader::new(3);
        reader.apply_page(&[]);
        assert_eq!(reader.orders.len(), 0);
    }

    #[test]
    fn auction_data_reader_single_batch() {
        let mut bytes = Vec::new();
        bytes.extend(ORDER_1_BYTES);
        bytes.extend(ORDER_2_BYTES);
        let mut reader = IndexedAuctionDataReader::new(3);
        reader.apply_page(&bytes);

        let mut account_state = AccountState::default();
        account_state.increase_balance(Address::from_low_u64_be(1), 257, 4);
        account_state.increase_balance(Address::from_low_u64_be(1), 258, 5);

        assert_eq!(reader.account_state, account_state);
        assert_eq!(reader.orders, [ORDER_1.clone(), ORDER_2.clone()]);
    }

    #[test]
    fn auction_data_reader_multiple_batches() {
        let mut account_state = AccountState::default();
        let mut reader = IndexedAuctionDataReader::new(3);
        let mut bytes = Vec::new();

        bytes.extend(ORDER_1_BYTES);
        reader.apply_page(&bytes);
        account_state.increase_balance(Address::from_low_u64_be(1), 257, 4);
        assert_eq!(reader.account_state, account_state);
        assert_eq!(reader.orders, [ORDER_1.clone()]);

        bytes.clear();
        bytes.extend(ORDER_2_BYTES);
        reader.apply_page(&bytes);
        account_state.increase_balance(Address::from_low_u64_be(1), 258, 5);
        assert_eq!(reader.account_state, account_state);
        assert_eq!(reader.orders, [ORDER_1.clone(), ORDER_2.clone()]);

        bytes.clear();
        bytes.extend(ORDER_3_BYTES);
        reader.apply_page(&bytes);
        account_state.increase_balance(Address::from_low_u64_be(2), 257, 6);
        assert_eq!(reader.account_state, account_state);
    }

    #[test]
    fn auction_data_reader_multiple_batches_different_users() {
        let mut account_state = AccountState::default();
        let mut reader = IndexedAuctionDataReader::new(3);
        let mut bytes = Vec::new();

        bytes.extend(ORDER_1_BYTES);
        reader.apply_page(&bytes);
        account_state.increase_balance(Address::from_low_u64_be(1), 257, 4);
        assert_eq!(reader.account_state, account_state);
        assert_eq!(reader.orders, [ORDER_1.clone()]);

        bytes.clear();
        bytes.extend(ORDER_2_BYTES);
        bytes.extend(ORDER_3_BYTES);
        reader.apply_page(&bytes);
        account_state.increase_balance(Address::from_low_u64_be(1), 258, 5);
        account_state.increase_balance(Address::from_low_u64_be(2), 257, 6);
        assert_eq!(reader.account_state, account_state);
    }

    #[test]
    fn auction_data_reader_order_count_does_not_ignore_filtered_orders() {
        let mut bytes = Vec::new();
        bytes.extend(ORDER_1_BYTES);
        bytes.extend(ORDER_2_BYTES);
        let mut reader = IndexedAuctionDataReader::new(1000);
        // the bytes contain two orders
        reader.apply_page(&bytes);

        // We don't include balances for orders that were filtered out
        assert_eq!(reader.account_state, AccountState::default());
        // orders is empty because `index` does not match
        assert_eq!(reader.orders, []);
    }
}
