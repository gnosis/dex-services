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
