# Pricegraph

Manipulate and inspect a Gnosis Protocol orderbook. This crate provides an
optimized graph based model for computing transitive orders (i.e. ring trades).
This can be used to provide orderbook spreads as well as price and exchange
rate estimates.

## Benchmarking

In order to benchmark a change, first run the benchmarking suite on the `master`
branch. From the root of the repository:

```
$ git checkout master
$ cargo bench -p pricegraph
...
Pricegraph::transitive_orderbook/5298183
                        time:   [3.4575 ms 3.5446 ms 3.6322 ms]
...
```

This will produce a report as well as store the current benchmark results so
they may be used for comparison with the version you are trying to benchmark.
Now checkout the commit which you are trying to compare to the current
implementation and run the benchmarking suite again. This time, notice that the
results contain information about the change in performance:

```
$ git checkout my-change
$ cargo bench -p pricegraph
...
Pricegraph::transitive_orderbook/5298183
                        time:   [3.1746 ms 3.2042 ms 3.2353 ms]
                        change: [-11.269% -9.0951% -6.9077%] (p = 0.00 < 0.05)
                        Performance has improved.
...
```

## Fuzzing

This crate can be fuzzed with [cargo fuzz](https://github.com/rust-fuzz/cargo-fuzz).

Fuzzing requires nightly which can be installed with `rustup install nightly`.
Then install *cargo fuzz* with `cargo +nightly install cargo-fuzz`.

List fuzz targets with `cargo +nightly fuzz list`.

Run a fuzz target with `cargo +nightly fuzz run orderbook`.

## Test Data

This crate contains test data from real orderbooks captured on `mainnet` in the
`data` subdirectory.

### Fetching the Current Orderbook

The `data` subdirectory contains a `fetch` script for fetching the current
orderbook. It can be executed from the repository root:

```
$ INFURA_PROJECT_ID=... cargo run -p pricegraph-data --bin fetch
[2020-08-10T07:45:09Z INFO  fetch] retrieving orderbook at block 10630913 until batch 5323483
[2020-08-10T07:45:09Z DEBUG fetch] retrieving page 0x0000…0000-0
[2020-08-10T07:45:10Z DEBUG fetch] retrieving page 0x7b2e…a196-24
[2020-08-10T07:45:11Z DEBUG fetch] retrieving page 0xb738…a513-10
...
```

This will add a new `orderbook-$BATCH_ID.hex` file to the `data` directory where
`$BATCH_ID` corresponds to the current batch ID (here `orderbook-5323483.hex`)
with the orderbook in a permissive hex encoded format that allows arbitrary
whitespace with lines to separate orders and spaces to separate fields.

### Converting a Solver Instance File

Additionally, solver instance files may be converted to an orderbook file with
the `convert` script. It can be executed from the repository root:

```
$ cargo run -p pricegraph-data --bin convert -- instance.json 123456789
[2020-08-10T11:18:32Z INFO  convert] encoding 13766 orders from `target/instance.json`
```

This will add a new `orderbook-$BATCH_ID.hex` file to the `data` directory where
`$BATCH_ID` corresponds to the batch ID specified on the command line (here
`orderbook-123456789.hex`. Again, the orderbook will be converted in the same
permissive hex format.

### Adding Test Data

Orderbook files generated with one of the above two scripts can can be added to
`data/mod.rs` by its batch ID so that it can be accessed from `pricegraph` tests
and benchmarks:

```diff
diff --git a/pricegraph/data/mod.rs b/pricegraph/data/mod.rs
index b6fb122..de2e7e5 100644
--- a/pricegraph/data/mod.rs
+++ b/pricegraph/data/mod.rs
@@ -29,6 +29,7 @@ lazy_static! {
 
         add_orderbook!(5298183);
         add_orderbook!(5301531);
+        add_orderbook!(123456789);
 
         orderbooks
     };
```
