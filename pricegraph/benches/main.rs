use criterion::{black_box, criterion_group, criterion_main, BatchSize, BenchmarkId, Criterion};
use pricegraph::{Orderbook, TokenPair};

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

pub fn orderbook_reduce_overlapping_orders(c: &mut Criterion) {
    c.bench_function("Orderbook::reduce_overlapping_orders", |b| {
        let orderbook = data::read_default_orderbook();
        b.iter_batched(
            || orderbook.clone(),
            |mut orderbook| orderbook.reduce_overlapping_orders(),
            BatchSize::SmallInput,
        )
    });
}

pub fn orderbook_fill_market_order(c: &mut Criterion) {
    let dai_weth = TokenPair { buy: 7, sell: 1 };
    let eth = 10.0f64.powi(18);
    let volumes = &[0.1 * eth, eth, 10.0 * eth, 100.0 * eth, 1000.0 * eth];

    let mut group = c.benchmark_group("Orderbook::fill_market_order");
    for volume in volumes {
        group.bench_with_input(BenchmarkId::from_parameter(volume), volume, |b, &volume| {
            let orderbook = data::read_default_orderbook();
            b.iter_batched(
                || orderbook.clone(),
                |mut orderbook| orderbook.fill_market_order(black_box(dai_weth), volume),
                BatchSize::SmallInput,
            )
        });
    }
    group.finish();

    let mut group = c.benchmark_group("Orderbook::fill_market_order(reduced)");
    for volume in volumes {
        group.bench_with_input(BenchmarkId::from_parameter(volume), volume, |b, &volume| {
            let reduced_orderbook = {
                let mut orderbook = data::read_default_orderbook();
                orderbook.reduce_overlapping_orders();
                orderbook
            };
            b.iter_batched(
                || reduced_orderbook.clone(),
                |mut orderbook| orderbook.fill_market_order(black_box(dai_weth), volume),
                BatchSize::SmallInput,
            )
        });
    }
    group.finish();
}

criterion_group!(
    benches,
    orderbook_read,
    orderbook_is_overlapping,
    orderbook_reduce_overlapping_orders,
    orderbook_fill_market_order,
);
criterion_main!(benches);
