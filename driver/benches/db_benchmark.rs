#[macro_use]
extern crate criterion;

extern crate rand;
use rand::random;

use criterion::Criterion;
use driver::persisted_merkle_tree::PersistedMerkleTree;

fn criterion_benchmark(c: &mut Criterion) {
    c.bench_function_over_inputs("update_record", |b, &&size| {
        let tree = PersistedMerkleTree::new(256, size);
        let mut counter = 0;
        b.iter(|| {
            tree.update_record(random(), |_| random::<u32>().to_string(), counter);
            counter = counter + 1;
        });
    }, &[1,2,4,8]);
}

criterion_group!(benches, criterion_benchmark);
criterion_main!(benches);