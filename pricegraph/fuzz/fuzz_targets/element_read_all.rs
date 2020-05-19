#![no_main]
use libfuzzer_sys::fuzz_target;
use pricegraph::Element;

// Fuzz Element::read_all .
// This is unlikely to find any panic because the implementation is simple.

fuzz_target!(|data: &[u8]| {
    if let Ok(elements) = Element::read_all(data) {
        // Iterate to consume the iterator.
        for _ in elements {}
    }
});
