# Fuzzing

This crate can be fuzzed with [cargo fuzz](https://github.com/rust-fuzz/cargo-fuzz).

Fuzzing requires nightly which can be installed with `rustup install nightly`. Then install *cargo fuzz* with `cargo +nightly install cargo-fuzz`.

List fuzz targets with `cargo +nightly fuzz list`.

Run a fuzz target with `cargo +nightly fuzz run orderbook`.