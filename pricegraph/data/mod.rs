//! Module containing test orderbook data.

use super::Orderbook;
use data_encoding::{Encoding, Specification};
use lazy_static::lazy_static;
use std::collections::BTreeMap;

lazy_static! {
    /// A permissive hex encoding that allows for whitespace.
    pub static ref HEX: Encoding = {
        let mut spec = Specification::new();
        spec.symbols.push_str("0123456789abcdef");
        spec.ignore.push_str(" \n");
        spec.encoding().unwrap()
    };

    /// The raw encoded test orderbooks that were retrieved from the mainnet
    /// smart contract for testing.
    pub static ref ORDERBOOKS: BTreeMap<usize, Vec<u8>> = {
        let mut orderbooks = BTreeMap::new();

        macro_rules! add_orderbook {
            ($batch:tt) => {
                #[allow(clippy::unreadable_literal)]
                orderbooks.insert($batch, HEX.decode(include_bytes!(
                    concat!("orderbook-", stringify!($batch), ".hex"),
                )).unwrap());
            }
        };

        add_orderbook!(5287195);

        orderbooks
    };
}

/// Reads a test orderbook by batch ID.
pub fn read_orderbook(batch_id: usize) -> Orderbook {
    Orderbook::read(&ORDERBOOKS[&batch_id]).expect("error reading orderbook")
}

/// Reads the default test orderbook.
pub fn read_default_orderbook() -> Orderbook {
    read_orderbook(5_287_195)
}
