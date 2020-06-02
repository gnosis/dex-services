#![no_main]
use libfuzzer_sys::fuzz_target;
use pricegraph::{Element, Orderbook};

// Fuzz target to test all pricegraph parts (run continuously on each commit).

fuzz_target!(|data: (Vec<Element>, Vec<u8>)| {
    let _ = Orderbook::from_elements(data.0);
    let elements = Element::read_all(&data.1);
    if let Ok(elements) = elements {
        // Iterate to consume the iterator.
        for _ in elements {}
    }
});
