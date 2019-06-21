#[macro_use]
extern crate criterion;

#[macro_use]
extern crate itertools;

extern crate rand;
use rand::random;

use criterion::Criterion;
use driver::persisted_merkle_tree::PersistedMerkleTree;

fn criterion_benchmark(c: &mut Criterion) {
    c.bench_function_over_inputs("update 1000 records", |b, &size| {
        let tree = PersistedMerkleTree::new(*size.0, *size.1);
        let mut counter = 0;
        b.iter(|| {
            let indices: Vec<u32> = (0..1000).map(|_|random()).collect();
            tree.update_records(indices, |_, _| random::<u32>().to_string(), counter);
            counter = counter + 1;
        });
    }, iproduct!(&[24, 32, 256], &[1,2,4,8]).collect::<Vec<(&u32, &u32)>>());
}

criterion_group!(benches, criterion_benchmark);
criterion_main!(benches);