## API

All endpoints use the query part of the url with these key-values:

* `atoms`: Required. If set to `true` all amounts are denominated in the smallest available unit (base quantity) of the token. If `false` all amounts are denominated in the "natural" unit of the respective token given by the number of decimals specified through the ERC20 interface. TODO: `false` is currently only implemented for estimated-buy-amount and estimated-amounts-at-price .
* `hops`: Optional. If provided, the exchange rate estimates are computed with a restricted maximum number of orders touched in the same sequence. Note that the restriction does not apply to orders that will be matched in the current batch.
* `batchId`: Optional. Specify a specific batch ID to compute the estimate for, only accounting orders that are valid at the specified batch. If no batch ID is specified, the current batch that is collecting orders will be used.

Example: `<path>?atoms=true`

The endpoint documentation references these types:

* A *token id* is a natural number >= 0 in decimal notation. It refers to the token ids in the smart contract. Example: `0`
* A *token amount* is given as a floating point number formatted according to https://doc.rust-lang.org/std/primitive.f64.html#impl-FromStr which resembles numbers in json closely. Examples: `0`, `1.1`.
* A *market* is of the form `<base token id>-<quote token id>`. Example: `0-1`

The service exposes the following endpoints:

### Markets

`GET /api/v1/markets/:market`

Example Request: `/api/v1/markets/1-7?atoms=true`

Example Response:

```json
{
    "asks": [
        { "price": 407.6755405630054, "volume": 9.389082650375993 }
    ],
    "bids": [
        { "price": 5508028446685.359, "volume": 3.2264600472733105 }
    ],
}
```

Returns the transitive orderbook (containing bids and asks) for the given base and quote token.

### Estimated Buy Amount

`GET /api/v1/markets/:market/estimated-buy-amount/:sell-amount-in-quote-token`

Example Request: `/api/v1/markets/1-7/estimated-buy-amount/20000000000000000000?atoms=true`

Example Response:

```json
{
    "baseTokenId": 1,
    "quoteTokenId": 7,
    "buyAmountInBase": "79480982311034354",
    "sellAmountInQuote": "20000000000000000000",
}
```

* `buyAmountInBase` estimates the buy amount (in base tokens) a user can set as a limit order while still expecting to be completely matched when selling the given amount of quote token.
* The other fields repeat the parameters in the url back.

### Estimated Amounts At Price

`GET /api/v1/markets/:market/estimated-amounts-at-price/:price-in-quote`

Example Request: `/api/v1/markets/1-7/estimated-amounts-at-price/245.5?atoms=true`

Example Response:

```json
{
    "baseTokenId": 1,
    "quoteTokenId": 7,
    "buyAmountInBase": "6098377078823660544",
    "sellAmountInQuote": "1497151572851208749056"
}
```

The following result indicates that if we wanted to buy ETH (token 2) for DAI (token 7) and pay 245.5 DAI per unit of ETH, we would be able to sell an estimated maximum 1497.15 DAI.

* `sellAmountInBase` estimates the sell amount (in quote tokens) a user can completely fill in the following batch at the specified `price_in_quote`.
* `buyAmountInBase` is the computed buy amount (in base tokens) for the order from the specified price and estimated sell amount. Note that it might be possible to use a higher buy amount for the same returned sell amount and still likely get completely matched by the solver. This buy amount can be computed with a subsequent estimate buy amount API call using the returned sell amount in quote value.
* The other fields repeat the parameters in the url back.

### Estimated Best Ask Price

`GET /api/v1/markets/:market/estimated-best-ask-price`

Example Request: `/api/v1/markets/1-7/estimated-best-ask-price?atoms=true`

Example Responses:

```json
297.8
```

The response is a json number or `null`.
It represents the exchange rate for the market. In the example we can exchange ~300 DAI (token 7) for 1 ETH (token 1). Note that the true exchange rate depends on the buy amount whereas this exchange rate is for a theoretical 0 amount.

# Testing

To test a locally running price estimator with the frontend at https://mesa.eth.link/ we need to set our browser to allow websites to access localhost and change the url that the javascript uses for the price estimator. With chromium:

1. `chromium --disable-web-security --user-data-dir=temp/`.
2. Open the frontend.
3. Open the developer tools with `F12`.
4. In the browser console enter `dexPriceEstimatorApi.urlsByNetwork[1] = "http://localhost:8080/api/v1/"`.
5. Induce a request by changing the sell amount and check that price estimator prints that it handled the request.

It is useful to start the price estimator with logging enabled, using the gnosis staging node url and using a permanent orderbook file:

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
