# Pricegraph Wasm Bindings

This crate provides WebAssembly bindings to the `pricegraph` crate so that it
can be used from JS environments.

## Usage

```js
const estimator = new PriceEstimator(encodedOrders);
const [WETH, DAI] = [1, 7];
console.log(estimator.estimatePrice(WETH, DAI, 100 * 10e18));
esimator.free();
```

## Building

This crate and the resulting npm package are created using
[`wasm-pack`](https://rustwasm.github.io/wasm-pack/).

To run integration tests inside a NodeJS environment:
```sh
cd pricegraph/wasm
wasm-pack test --node
```

To generate the `pkg/` directory containing the npm package:
```sh
cd pricegraph/wasm
wasm-pack build --scope gnosis.pm --target nodejs
```
