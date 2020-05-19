#![no_main]
use libfuzzer_sys::fuzz_target;
use pricegraph::{Element, Orderbook};

// Fuzz creation and usage of Orderbook.

fuzz_target!(|elements: Vec<Element>| {
    let _ = Orderbook::from_elements(elements);
});
