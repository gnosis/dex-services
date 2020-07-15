#[path = "../data/mod.rs"]
mod data;

use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion};
use pricegraph::{Market, Pricegraph, TokenPair};

fn read_default_pricegraph() -> Pricegraph {
    Pricegraph::read(&*data::DEFAULT_ORDERBOOK).expect("error reading orderbook")
}

pub fn read(c: &mut Criterion) {
    c.bench_function("Pricegraph::read", |b| b.iter(read_default_pricegraph));
}

pub fn transitive_orderbook(c: &mut Criterion) {
    let pricegraph = read_default_pricegraph();
    let dai_weth = Market { base: 7, quote: 1 };

    c.bench_with_input(
        BenchmarkId::new("Pricegraph::transitive_orderbook", *data::DEFAULT_BATCH_ID),
        &(&pricegraph, dai_weth),
        |b, &(pricegraph, dai_weth)| b.iter(|| pricegraph.transitive_orderbook(dai_weth, None)),
    );
}

pub fn estimate_limit_price(c: &mut Criterion) {
    let pricegraph = read_default_pricegraph();
    let dai_weth = TokenPair { buy: 7, sell: 1 };
    let eth = 10.0f64.powi(18);
    let volumes = &[0.1 * eth, eth, 10.0 * eth, 100.0 * eth, 1000.0 * eth];

    let mut group = c.benchmark_group("Pricegraph::estimate_limit_price");
    for volume in volumes {
        group.bench_with_input(
            BenchmarkId::from_parameter(volume),
            &(&pricegraph, dai_weth, *volume),
            |b, &(pricegraph, pair, volume)| {
                b.iter(|| pricegraph.estimate_limit_price(pair, volume))
            },
        );
    }
    group.finish();
}

pub fn order_for_limit_price(c: &mut Criterion) {
    let pricegraph = read_default_pricegraph();
    let dai_weth = TokenPair { buy: 7, sell: 1 };
    let prices = &[200.0, 190.0, 180.0, 150.0, 100.0];

    let mut group = c.benchmark_group("Pricegraph::order_for_limit_price");
    for price in prices {
        group.bench_with_input(
            BenchmarkId::from_parameter(price),
            &(&pricegraph, dai_weth, *price),
            |b, &(pricegraph, pair, price)| {
                b.iter(|| pricegraph.order_for_limit_price(pair, price))
            },
        );
    }
    group.finish();
}

criterion_group!(
    name = benches;
    config = Criterion::default().sample_size(50);
    targets = read, transitive_orderbook, estimate_limit_price, order_for_limit_price
);
criterion_main!(benches);
