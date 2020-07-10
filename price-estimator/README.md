## API

All endpoints use the query part of the url with these key-values:

* `atoms`: Required. If set to `true` all amounts are denominated in the smallest available unit (base quantity) of the token. If `false` all amounts are denominated in the "natural" unit of the respective token given by the number of decimals specified through the ERC20 interface. TODO: `false` is currently only implemented for estimated-buy-amount and estimated-amounts-at-price .
* `hops`: Optional. TODO: document this once it has been implemented.

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
    "baseTokenId": "1",
    "quoteTokenId": "7",
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
    "baseTokenId": "1",
    "quoteTokenId": "7",
    "buyAmountInBase": "6098377078823660544",
    "sellAmountInQuote": "1497151572851208749056"
}
```

The following result indicates that if we wanted to buy ETH (token 2) for DAI (token 7) and pay 245.5 DAI per unit of ETH, we would be able to sell an estimated maximum 1497.15 DAI.

* `sellAmountInBase` estimates the sell amount (in quote tokens) a user can completely fill in the following batch at the specified `price_in_quote`.
* `buyAmountInBase` is the computed buy amount (in base tokens) for the order from the specified price and estimated sell amount. Note that it might be possible to use a higher buy amount for the same returned sell amount and still likely get completely matched by the solver. This buy amount can be computed with a subsequent estimate buy amount API call using the returned sell amount in quote value.
* The other fields repeat the parameters in the url back.
