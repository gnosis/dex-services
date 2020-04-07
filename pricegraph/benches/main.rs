use criterion::{black_box, criterion_group, criterion_main, Criterion};
use pricegraph::Orderbook;

/// Module containing test orderbook data.
#[path = "../data/mod.rs"]
pub mod data;

pub fn orderbook_read(c: &mut Criterion) {
    c.bench_function("Orderbook::read", |b| {
        b.iter(|| Orderbook::read(&data::ORDERBOOKS[0]).expect("error reading orderbook"))
    });
}

pub fn orderbook_is_overlapping(c: &mut Criterion) {
    let orderbook = Orderbook::read(&data::ORDERBOOKS[0]).expect("error reading orderbook");

    c.bench_function("Orderbook::is_overlapping", |b| {
        b.iter(|| black_box(&orderbook).is_overlapping())
    });
}

criterion_group!(benches, orderbook_read, orderbook_is_overlapping);
criterion_main!(benches);
