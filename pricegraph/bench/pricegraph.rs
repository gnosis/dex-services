use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion};
use pricegraph::{Market, Pricegraph, TokenPair};
use pricegraph_data::{DEFAULT_ORDERBOOK};
use std::time::Duration;

fn read_default_pricegraph() -> Pricegraph {
    Pricegraph::read(&*DEFAULT_ORDERBOOK).expect("error reading orderbook")
}

pub fn read(c: &mut Criterion) {
    c.bench_function("Pricegraph::read", |b| b.iter(read_default_pricegraph));
}

pub fn transitive_orderbook(c: &mut Criterion) {
    let pricegraph = read_default_pricegraph();
    let dai_weth = Market { base: 7, quote: 1 };
    let hops = &[None, Some(1), Some(2), Some(5), Some(10), Some(30)];

    let mut group = c.benchmark_group("Pricegraph::transitive_orderbook_with_hops");
    for hops in hops {
        group.bench_with_input(
            BenchmarkId::from_parameter(format!("{:?}", hops)),
            &(&pricegraph, dai_weth, hops),
            |b, &(pricegraph, dai_weth, hops)| {
                b.iter(|| pricegraph.transitive_orderbook(dai_weth, *hops, None))
            },
        );
    }
    group.finish();
}

pub fn estimate_limit_price(c: &mut Criterion) {
    let pricegraph = read_default_pricegraph();
    let dai_weth = TokenPair { buy: 7, sell: 1 }.into_unbounded_range();
    let eth = 1e18;
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
    let dai_weth = TokenPair { buy: 7, sell: 1 }.into_unbounded_range();
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
    name = overlapping;
    config = Criterion::default().measurement_time(Duration::from_secs(60));
    targets = read, transitive_orderbook
);
criterion_group!(
    name = reduced;
    config = Criterion::default().measurement_time(Duration::from_secs(10));
    targets =  estimate_limit_price, order_for_limit_price
);
criterion_main!(overlapping, reduced);
