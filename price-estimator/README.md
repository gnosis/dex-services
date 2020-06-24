## API

The service exposes the following endpoints:

### Markets

`GET /api/v1/markets/:market`

* `market` is of the form `<base_token_id>-<quote_token_id>`. The token ids the same as in the smart contract.

Url Query:
* `atoms`: If set to `true` (for now this is the only implemented method) all amounts will be denominated in the smallest available unit (base quantity) of the token.
* `hops`: TODO: document this once it has been implemented.

Example Request: `/api/v1/markets/1-7/?atoms=true`

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

* `market` is as above
* `sell_amount_in_quote_token` is a positive integer.

Url query is as above.

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

### Estimated Sell Amount

`GET /api/v1/markets/:market/estimated-sell-amount/:price-in-quote`

* `market` is as above
* `price_in_quote` is a decimal number.

Url query is as above.

Example Request: `/api/v1/markets/1-7/estimated-sell-amount/245.5?atoms=true`

Example Response:

```json
{
    "baseTokenId": "1",
    "quoteTokenId": "7",
    "buyAmountInBase": "1103276584712580736",
    "sellAmountInQuote": "271125527074012594176"
}
```

The following result indicates that if we wanted to buy ETH (token 1) for DAI (token 7) and pay 245.5 DAI per unit of ETH, we would be able to sell an estimated maximum 271.13 DAI.

* `sellAmountInBase` estimates the sell amount (in quote tokens) a user can completely fill in the following batch at the specified `price_in_quote`.
* `buyAmountInBase` is the computed buy amount (in base tokens) for the order from the specified price and estimated sell amount.
* The other fields repeat the parameters in the url back.
