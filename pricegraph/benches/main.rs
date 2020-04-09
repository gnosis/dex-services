use criterion::{criterion_group, criterion_main, BatchSize, Criterion};
use pricegraph::Orderbook;

#[path = "../data/mod.rs"]
pub mod data;

pub fn orderbook_read(c: &mut Criterion) {
    c.bench_function("Orderbook::read", |b| b.iter(data::read_default_orderbook));
}

pub fn orderbook_is_overlapping(c: &mut Criterion) {
    let orderbook = data::read_default_orderbook();

    c.bench_function("Orderbook::is_overlapping", |b| {
        b.iter(|| orderbook.is_overlapping())
    });
}

pub fn orderbook_update_projection_graph(c: &mut Criterion) {
    c.bench_function("Orderbook::update_projection_graph", |b| {
        b.iter_batched(
            data::read_default_orderbook,
            |mut orderbook| orderbook.update_projection_graph(),
            BatchSize::SmallInput,
        )
    });
}

criterion_group!(
    benches,
    orderbook_read,
    orderbook_is_overlapping,
    orderbook_update_projection_graph,
);
criterion_main!(benches);
