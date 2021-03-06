//! Module containing test orderbook data.

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
    pub static ref ORDERBOOKS: BTreeMap<u32, Vec<u8>> = {
        let mut orderbooks = BTreeMap::new();

        macro_rules! add_orderbook {
            ($batch:tt) => {
                #[allow(clippy::unreadable_literal)]
                orderbooks.insert($batch, HEX.decode(include_bytes!(
                    concat!("orderbook-", stringify!($batch), ".hex"),
                )).unwrap());
            }
        };

        add_orderbook!(5298183);
        add_orderbook!(5301531);

        orderbooks
    };

    /// The default batch ID used for testing and benchmarking.
    pub static ref DEFAULT_BATCH_ID: u32 = 5_298_183;

    /// The default orderbook used for testing and benchmarking.
    pub static ref DEFAULT_ORDERBOOK: &'static [u8] = &*ORDERBOOKS[&*DEFAULT_BATCH_ID];
}
