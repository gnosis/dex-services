#[path = "../data/mod.rs"]
mod data;

use criterion::{black_box, criterion_group, criterion_main, BatchSize, BenchmarkId, Criterion};
use pricegraph::{Orderbook, TokenPair};

pub fn read_default_orderbook() -> Orderbook {
    Orderbook::read(&*data::DEFAULT_ORDERBOOK).expect("error reading orderbook")
}

pub fn read(c: &mut Criterion) {
    c.bench_function("Orderbook::read", |b| b.iter(read_default_orderbook));
}

pub fn is_overlapping(c: &mut Criterion) {
    let orderbook = read_default_orderbook();

    c.bench_function("Orderbook::is_overlapping", |b| {
        b.iter(|| orderbook.is_overlapping())
    });
}

pub fn reduce_overlapping_orders(c: &mut Criterion) {
    c.bench_function("Orderbook::reduce_overlapping_orders", |b| {
        let orderbook = read_default_orderbook();
        b.iter_batched(
            || orderbook.clone(),
            |mut orderbook| orderbook.reduce_overlapping_orders(),
            BatchSize::SmallInput,
        )
    });
}

pub fn reduce_overlapping_transitive_orderbook(c: &mut Criterion) {
    c.bench_function("Orderbook::reduce_overlapping_transitive_orderbook", |b| {
        let orderbook = read_default_orderbook();
        b.iter_batched(
            || orderbook.clone(),
            |mut orderbook| orderbook.reduce_overlapping_transitive_orderbook(1, 7),
            BatchSize::SmallInput,
        )
    });
}

pub fn fill_transitive_orders(c: &mut Criterion) {
    let dai_weth = TokenPair { buy: 7, sell: 1 };
    let spreads = &[Some(0.1), Some(0.25), Some(0.5), Some(1.0), None];

    let mut group = c.benchmark_group("Orderbook::fill_transitive_orders");
    for spread in spreads {
        group.bench_with_input(
            BenchmarkId::from_parameter(spread.unwrap_or(f64::INFINITY)),
            spread,
            |b, &spread| {
                let orderbook = read_default_orderbook();
                b.iter_batched(
                    || orderbook.clone(),
                    |mut orderbook| orderbook.fill_transitive_orders(black_box(dai_weth), spread),
                    BatchSize::SmallInput,
                )
            },
        );
    }
    group.finish();

    let mut group = c.benchmark_group("Orderbook::fill_transitive_orders(reduced)");
    for spread in spreads {
        group.bench_with_input(
            BenchmarkId::from_parameter(spread.unwrap_or(f64::INFINITY)),
            spread,
            |b, &spread| {
                let reduced_orderbook = {
                    let mut orderbook = read_default_orderbook();
                    orderbook.reduce_overlapping_orders();
                    orderbook
                };
                b.iter_batched(
                    || reduced_orderbook.clone(),
                    |mut orderbook| orderbook.fill_transitive_orders(black_box(dai_weth), spread),
                    BatchSize::SmallInput,
                )
            },
        );
    }
    group.finish();
}

pub fn fill_market_order(c: &mut Criterion) {
    let dai_weth = TokenPair { buy: 7, sell: 1 };
    let eth = 10.0f64.powi(18);
    let volumes = &[0.1 * eth, eth, 10.0 * eth, 100.0 * eth, 1000.0 * eth];

    let mut group = c.benchmark_group("Orderbook::fill_market_order");
    for volume in volumes {
        group.bench_with_input(BenchmarkId::from_parameter(volume), volume, |b, &volume| {
            let orderbook = read_default_orderbook();
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
                let mut orderbook = read_default_orderbook();
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
    name = benches;
    config = Criterion::default().sample_size(20);
    targets =
        read, is_overlapping, reduce_overlapping_orders,
        reduce_overlapping_transitive_orderbook, fill_transitive_orders,
        fill_market_order
);
criterion_main!(benches);
