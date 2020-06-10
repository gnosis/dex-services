## API

The service exposes the following endpoints:

### estimated buy amount

`GET /markets/:market/estimated-buy-amount/:sell-amount-in-quote-token`

* `market` is of the form `<base_token_id>-<quote_token_id>`. The token ids the same as in the smart contract.
* `sell_amount_in_quote_token` is a positive integer.

Url Query:
* `atoms`: If set to `true` (for now this is the only implemented method) all amounts will be denominated in the smallest available unit (base quantity) of the token.
* `hops`: TODO: document this once it has been implemented.

Example Request: `markets/1-7/estimated-buy-amount/20000000000000000000?atoms=true`

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
