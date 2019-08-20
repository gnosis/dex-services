#!/bin/bash

cd dex-contracts
source ../test/utils.sh

step "Setup" \
"npx truffle exec scripts/setup_environment.js 6"

EXPECTED_AUCTION=0

step "Make sure we have enough balances for the trades" \
"npx truffle exec scripts/deposit.js 0 2 300 && \
 npx truffle exec scripts/deposit.js 1 1 300 && \
 npx truffle exec scripts/deposit.js 2 2 200 && \
 npx truffle exec scripts/deposit.js 3 1 300 && \
 npx truffle exec scripts/deposit.js 4 0 300 && \
 npx truffle exec scripts/deposit.js 5 0 300"

step "Advance time to apply deposits" \
"npx truffle exec scripts/wait_seconds.js 181"

step "Place 6 orders in current Auction" \
"npx truffle exec scripts/sell_order.js 0 1 2 12 12 && \
 npx truffle exec scripts/sell_order.js 1 2 1 2.2 2 && \
 npx truffle exec scripts/sell_order.js 2 0 2 150 10 && \
 npx truffle exec scripts/sell_order.js 3 0 1 180 15 && \
 npx truffle exec scripts/sell_order.js 4 1 0 4 52  && \
 npx truffle exec scripts/sell_order.js 5 2 0 20 280"

step_with_retry "[theGraph] SellOrder was added to graph db - accountId 5's sellOrder === 280" \
"source ../test/utils.sh && query_graphql \
    \"query { \
        sellOrders(where: { accountId: 5}) { \
            sellAmount \
        } \
    }\" | grep 28000000000000000000"

step "Advance time to apply auction" \
"npx truffle exec scripts/wait_seconds.js 181"

EXPECTED_HASH="2b87dc830d051be72f4adcc3677daadab2f3f2253e9da51d803faeb0daa1532f"
step_with_retry "Test that balances have been updated" \
"npx truffle exec scripts/invokeViewFunction.js 'getCurrentStateRoot' | grep ${EXPECTED_HASH}"

step_with_retry "[theGraph] Account 4 has now 4 of token 1" \
"source ../test/utils.sh && query_graphql \
    \"query { \
        accountStates(where: {id: \\\"${EXPECTED_HASH}\\\"}) {\
            balances \
        } \
    }\" | jq .data.accountStates[0].balances[121] | grep -w 4000000000000000000"

step_with_retry "[theGraph] Account 3 has now 52 of token 0" \
"source ../test/utils.sh && query_graphql \
    \"query { \
        accountStates(where: {id: \\\"${EXPECTED_HASH}\\\"}) {\
            balances \
        } \
    }\" | jq .data.accountStates[0].balances[90] | grep -w 52000000000000000000"