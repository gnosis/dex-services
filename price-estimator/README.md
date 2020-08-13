## API

The api is documented with [OpenAPI](https://www.openapis.org/) in `openapi.yml`. It can be accessed in a rendered and interactive fashion on https://editor.swagger.io/ through `File -> Import file`.

TODO: Add a binary or docker image that can host the interactive ui without an external service.

## Testing

To test a locally running price estimator with the frontend at https://mesa.eth.link/ we need to set our browser to allow websites to access localhost and change the URL that the javascript uses for the price estimator. With chromium:

1. `chromium --disable-web-security --user-data-dir=temp/`.
2. Open the frontend.
3. Open the developer tools with `F12`.
4. In the browser console enter `dexPriceEstimatorApi.urlsByNetwork[1] = "http://localhost:8080/api/v1/"`.
5. Induce a request by changing the sell amount and check that price estimator prints that it handled the request.

It is useful to start the price estimator with logging enabled, using the gnosis staging node URL and using a permanent orderbook file:

```
env RUST_LOG=warn,price_estimator=info,core=info cargo run -p price-estimator -- --node-url https://staging-openethereum.mainnet.gnosisdev.com --orderbook-file ../orderbook-file-mainnet
```

## Benchmarking

Benchmarking can be performed with your HTTP request benchmarking application of choice. For example using `autocannon` with `npx`:
```
$ cargo run -p price-estimator &
$ npx autocannon -c 1000 -d 300 'http://localhost:8080/api/v1/markets/1-7/estimated-buy-amount/1000000000000000000?atoms=true'
Running 300s test @ http://localhost:8080/api/v1/markets/1-7/estimated-buy-amount/100000000000000000000?atoms=true
1000 connections

┌─────────┬──────┬───────┬────────┬────────┬───────┬──────────┬────────────┐
│ Stat    │ 2.5% │ 50%   │ 97.5%  │ 99%    │ Avg   │ Stdev    │ Max        │
├─────────┼──────┼───────┼────────┼────────┼───────┼──────────┼────────────┤
│ Latency │ 3 ms │ 40 ms │ 163 ms │ 200 ms │ 51 ms │ 42.88 ms │ 1248.19 ms │
└─────────┴──────┴───────┴────────┴────────┴───────┴──────────┴────────────┘
┌───────────┬─────────┬─────────┬─────────┬─────────┬─────────┬─────────┬────────┐
│ Stat      │ 1%      │ 2.5%    │ 50%     │ 97.5%   │ Avg     │ Stdev   │ Min    │
├───────────┼─────────┼─────────┼─────────┼─────────┼─────────┼─────────┼────────┤
│ Req/Sec   │ 10695   │ 11375   │ 20943   │ 23311   │ 19420.7 │ 3485.85 │ 10442  │
├───────────┼─────────┼─────────┼─────────┼─────────┼─────────┼─────────┼────────┤
│ Bytes/Sec │ 2.46 MB │ 2.62 MB │ 4.82 MB │ 5.36 MB │ 4.47 MB │ 802 kB  │ 2.4 MB │
└───────────┴─────────┴─────────┴─────────┴─────────┴─────────┴─────────┴────────┘
```
