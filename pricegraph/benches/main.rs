use criterion::{criterion_group, criterion_main, Criterion};
use data_encoding::Specification;
use pricegraph::Orderbook;

pub fn orderbook_read(c: &mut Criterion) {
    let hex = {
        let mut spec = Specification::new();
        spec.symbols.push_str("0123456789abcdef");
        spec.ignore.push_str(" \n");
        spec.encoding().unwrap()
    };
    let encoded_orderbook = hex
        .decode(include_bytes!("../data/orderbook-5287195.hex"))
        .expect("orderbook contains invalid hex");

    c.bench_function("Orderbook::read", |b| {
        b.iter(|| Orderbook::read(&encoded_orderbook).expect("error reading orderbook"))
    });
}

criterion_group!(benches, orderbook_read);
criterion_main!(benches);
