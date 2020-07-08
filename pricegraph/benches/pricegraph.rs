#[path = "../data/mod.rs"]
mod data;

use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion};
use pricegraph::{Pricegraph, TokenPair};

fn read_default_pricegraph() -> Pricegraph {
    Pricegraph::read(&*data::DEFAULT_ORDERBOOK).expect("error reading orderbook")
}

pub fn read(c: &mut Criterion) {
    c.bench_function("Pricegraph::read", |b| b.iter(read_default_pricegraph));
}

pub fn transitive_orderbook(c: &mut Criterion) {
    let pricegraph = read_default_pricegraph();
    let base = 1;
    let quote = 7;

    c.bench_with_input(
        BenchmarkId::new("Pricegraph::transitive_orderbook", *data::DEFAULT_BATCH_ID),
        &(&pricegraph, base, quote),
        |b, &(pricegraph, base, quote)| {
            b.iter(|| pricegraph.transitive_orderbook(base, quote, None))
        },
    );
}

pub fn estimate_exchange_rate(c: &mut Criterion) {
    let pricegraph = read_default_pricegraph();
    let dai_weth = TokenPair { buy: 7, sell: 1 };
    let eth = 10.0f64.powi(18);
    let volumes = &[0.1 * eth, eth, 10.0 * eth, 100.0 * eth, 1000.0 * eth];

    let mut group = c.benchmark_group("Pricegraph::estimate_exchange_rate");
    for volume in volumes {
        group.bench_with_input(
            BenchmarkId::from_parameter(volume),
            &(&pricegraph, dai_weth, *volume),
            |b, &(pricegraph, pair, volume)| {
                b.iter(|| pricegraph.estimate_exchange_rate(pair, volume))
            },
        );
    }
    group.finish();
}

pub fn order_for_limit_exchange_rate(c: &mut Criterion) {
    let pricegraph = read_default_pricegraph();
    let dai_weth = TokenPair { buy: 7, sell: 1 };
    let prices = &[200.0, 190.0, 180.0, 150.0, 100.0];

    let mut group = c.benchmark_group("Pricegraph::order_for_limit_exchange_rate");
    for price in prices {
        group.bench_with_input(
            BenchmarkId::from_parameter(price),
            &(&pricegraph, dai_weth, *price),
            |b, &(pricegraph, pair, price)| {
                b.iter(|| pricegraph.order_for_limit_exchange_rate(pair, price))
            },
        );
    }
    group.finish();
}

criterion_group!(
    name = benches;
    config = Criterion::default().sample_size(50);
    targets = read, transitive_orderbook, estimate_exchange_rate, order_for_limit_exchange_rate
);
criterion_main!(benches);
