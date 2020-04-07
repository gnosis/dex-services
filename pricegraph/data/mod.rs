//! Module containing test orderbook data.

use data_encoding::{Encoding, Specification};
use lazy_static::lazy_static;

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
    pub static ref ORDERBOOKS: Vec<Vec<u8>> = vec![
        HEX.decode(include_bytes!("orderbook-5287195.hex")).unwrap(),
    ];
}
