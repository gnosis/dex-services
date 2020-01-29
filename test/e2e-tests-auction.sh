#!/bin/bash

cd dex-contracts
source ../test/utils.sh

step "Setup" \
"npx truffle exec scripts/snapp/setup_environment.js --numAccounts=6"

EXPECTED_AUCTION=0

step "Make sure we have enough balances for the trades" \
"npx truffle exec scripts/snapp/deposit.js --accountId=0 --tokenId=2 --amount=300 && \
 npx truffle exec scripts/snapp/deposit.js --accountId=1 --tokenId=1 --amount=300 && \
 npx truffle exec scripts/snapp/deposit.js --accountId=2 --tokenId=2 --amount=300 && \
 npx truffle exec scripts/snapp/deposit.js --accountId=3 --tokenId=1 --amount=300 && \
 npx truffle exec scripts/snapp/deposit.js --accountId=4 --tokenId=0 --amount=300 && \
 npx truffle exec scripts/snapp/deposit.js --accountId=5 --tokenId=0 --amount=300"

step "Advance time to apply deposits" \
"npx truffle exec scripts/wait_seconds.js 181"

step "Place 6 orders in current Auction" \
"npx truffle exec scripts/snapp/sell_order.js --accountId=0 --buyToken=1 --sellToken=2 --minBuy=12 --maxSell=12 && \
 npx truffle exec scripts/snapp/sell_order.js --accountId=1 --buyToken=2 --sellToken=1 --minBuy=2.2 --maxSell=2 && \
 npx truffle exec scripts/snapp/sell_order.js --accountId=2 --buyToken=0 --sellToken=2 --minBuy=150 --maxSell=10 && \
 npx truffle exec scripts/snapp/sell_order.js --accountId=3 --buyToken=0 --sellToken=1 --minBuy=180 --maxSell=15 && \
 npx truffle exec scripts/snapp/sell_order.js --accountId=4 --buyToken=1 --sellToken=0 --minBuy=4 --maxSell=52  && \
 npx truffle exec scripts/snapp/sell_order.js --accountId=5 --buyToken=2 --sellToken=0 --minBuy=20 --maxSell=280"

step_with_retry "[theGraph] SellOrder was added to graph db - accountId 5's sellOrder === 280" \
"source ../test/utils.sh && query_graphql \
    \"query { \
        sellOrders(where: { accountId: \\\"0000000000000000000000000000000000000005\\\" }) { \
            sellAmount \
        } \
    }\" | grep 28000000000000000000"

step "Advance time to bid for auction" \
"npx truffle exec scripts/wait_seconds.js 181"

EXPECTED_HASH="572dd059c22fe72a966510cba30961215c9e60b96359ccb79996ad3f9c1668f8"
step_with_retry "Wait for bid to be placed" \
"npx truffle exec scripts/snapp/invokeViewFunction.js auctions 0 | grep ${EXPECTED_HASH} "

step "Advance time to apply auction" \
"npx truffle exec scripts/wait_seconds.js 181"

step_with_retry "Test balances have been updated" \
"npx truffle exec scripts/snapp/invokeViewFunction.js 'getCurrentStateRoot' | grep ${EXPECTED_HASH}"

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