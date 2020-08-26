#[path = "../data/mod.rs"]
mod data;

use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion};
use pricegraph::{Market, Pricegraph, TokenPair, TokenPairRange};

const BENCHED_HOPS: [Option<u16>; 4] = [None, Some(2), Some(30), Some(u16::MAX)];

fn read_default_pricegraph() -> Pricegraph {
    Pricegraph::read(&*data::DEFAULT_ORDERBOOK).expect("error reading orderbook")
}

pub fn read(c: &mut Criterion) {
    c.bench_function("Pricegraph::read", |b| b.iter(read_default_pricegraph));
}

pub fn transitive_orderbook(c: &mut Criterion) {
    let pricegraph = read_default_pricegraph();
    let dai_weth = Market { base: 7, quote: 1 };

    let mut group = c.benchmark_group("Transitive orderbook");

    for &hops in &BENCHED_HOPS {
        group.bench_with_input(
            BenchmarkId::from_parameter(format!(
                "batch id: {}, hops: {:?}",
                *data::DEFAULT_BATCH_ID,
                hops
            )),
            &(&pricegraph, dai_weth),
            |b, &(pricegraph, dai_weth)| {
                b.iter(|| pricegraph.transitive_orderbook(dai_weth, hops, None))
            },
        );
    }
    group.finish();
}

pub fn estimate_limit_price(c: &mut Criterion) {
    let pricegraph = read_default_pricegraph();
    let dai_weth = TokenPair { buy: 7, sell: 1 };
    let eth = 1e18;
    let volumes = &[0.1 * eth, eth, 10.0 * eth, 100.0 * eth, 1000.0 * eth];

    let mut group = c.benchmark_group("Pricegraph::estimate_limit_price");
    for volume in volumes {
        for &hops in &BENCHED_HOPS {
            group.bench_with_input(
                BenchmarkId::from_parameter(format!("volume: {}, hops: {:?}", volume, hops)),
                &(&pricegraph, dai_weth, *volume),
                |b, &(pricegraph, pair, volume)| {
                    b.iter(|| {
                        pricegraph.estimate_limit_price(TokenPairRange { pair, hops }, volume)
                    })
                },
            );
        }
    }
    group.finish();
}

pub fn order_for_limit_price(c: &mut Criterion) {
    let pricegraph = read_default_pricegraph();
    let dai_weth = TokenPair { buy: 7, sell: 1 };
    let prices = &[200.0, 190.0, 180.0, 150.0, 100.0];

    let mut group = c.benchmark_group("Pricegraph::order_for_limit_price");
    for price in prices {
        for &hops in &BENCHED_HOPS {
            group.bench_with_input(
                BenchmarkId::from_parameter(format!("price: {}, hops: {:?}", price, hops)),
                &(&pricegraph, dai_weth, *price),
                |b, &(pricegraph, pair, price)| {
                    b.iter(|| {
                        pricegraph.order_for_limit_price(TokenPairRange { pair, hops }, price)
                    })
                },
            );
        }
    }
    group.finish();
}

criterion_group!(
    name = benches;
    config = Criterion::default().sample_size(50);
    targets = read, transitive_orderbook, estimate_limit_price, order_for_limit_price
);
criterion_main!(benches);
