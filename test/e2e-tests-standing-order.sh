#!/bin/bash

cd dex-contracts
source ../test/utils.sh

step "Setup" \
"npx truffle exec scripts/snapp/setup_environment.js"

step "Ensure sufficient Balances" \
"npx truffle exec scripts/snapp/deposit.js --accountId=0 --tokenId=2 --amount=300 && \
 npx truffle exec scripts/snapp/deposit.js --accountId=1 --tokenId=1 --amount=300 && \
 npx truffle exec scripts/wait_seconds.js 181"

step "Place Sell Order" \
"npx truffle exec scripts/snapp/sell_order.js --accountId=1 --buyToken=2 --sellToken=1 --minBuy=1 --maxSell=1"

step "Place standing order in current Auction" \
"npx truffle exec scripts/snapp/standing_order.js --accountId=0 --buyToken=1 --sellToken=2 --minBuy=1 --maxSell=1"

step_with_retry "Check graph standing order batch has been recorded" \
"source ../test/utils.sh && query_graphql \
    \"query { \
        standingSellOrderBatches(where: { \
          accountId: \\\"0000000000000000000000000000000000000000\\\" , \
          batchIndex: 0, \
          validFromAuctionId: 0 \
        }) { \
        orders { \
            buyToken \
            sellToken \
            buyAmount \
            sellAmount \
        } \
      } \
    }\" | grep \"buyAmount.:.1000000000000000000.,.buyToken.:1,.sellAmount.:.1000000000000000000.,.sellToken.:2\""

step "Advance time to bid for auction" \
"npx truffle exec scripts/wait_seconds.js 181"

EXPECTED_HASH="2b87dc830d051be72f4adcc3677daadab2f3f2253e9da51d803faeb0daa1532f"
step_with_retry "Wait for bid to be placed" \
"npx truffle exec scripts/snapp/invokeViewFunction.js auctions 0 | grep \"solver: '0x90F8bf6A479f320ead074411a4B0e7944Ea8c9C1'\" "

step "Advance time to apply auction" \
"npx truffle exec scripts/wait_seconds.js 181"

step_with_retry "Assert Standing order account traded" \
"source ../test/utils.sh && query_graphql \
    \"query { \
        accountStates(where: {stateIndex: \\\"2\\\"}) {\
            balances \
        } \
    }\" | jq .data.accountStates[0].balances[1] | grep -w -2 1000000000000000000"

step "Place matching sell order for standing order" \
"npx truffle exec scripts/snapp/sell_order.js --accountId=1 --buyToken=2 --sellToken=1 --minBuy=1 --maxSell=1"

step "Advance time to bid for auction" \
"npx truffle exec scripts/wait_seconds.js 181"

EXPECTED_HASH="2b87dc830d051be72f4adcc3677daadab2f3f2253e9da51d803faeb0daa1532f"
step_with_retry "Wait for bid to be placed" \
"npx truffle exec scripts/snapp/invokeViewFunction.js auctions 1 | grep \"solver: '0x90F8bf6A479f320ead074411a4B0e7944Ea8c9C1'\" "

step "Advance time to apply auction" \
"npx truffle exec scripts/wait_seconds.js 181"

step_with_retry "Make sure standing order is still traded" \
"source ../test/utils.sh && query_graphql \
    \"query { \
        accountStates(where: {stateIndex: \\\"3\\\"}) {\
            balances \
        } \
    }\" | jq .data.accountStates[0].balances[1] | grep -w -2 2000000000000000000"

step "Update standing order" \
"npx truffle exec scripts/snapp/standing_order.js --accountId=0 --buyToken=1 --sellToken=2 --minBuy=1 --maxSell=2"

step_with_retry "Check graph standing order batch has been updated" \
"source ../test/utils.sh && query_graphql \
    \"query { \
        standingSellOrderBatches(where: { \
          accountId: \\\"0000000000000000000000000000000000000000\\\" , \
          batchIndex: 1, \
          validFromAuctionId: 2 \
        }) { \
        orders { \
            buyToken \
            sellToken \
            buyAmount \
            sellAmount \
        } \
      } \
    }\" | grep \"buyAmount.:.1000000000000000000.,.buyToken.:1,.sellAmount.:.2000000000000000000.,.sellToken.:2\""

step "Cancel standing order in same batch (make sure only cancel gets processed)" \
"npx truffle exec scripts/snapp/standing_order.js --accountId=0 --buyToken=0 --sellToken=0 --minBuy=0 --maxSell=0"

step_with_retry "Check graph standing order batch has been deleted" \
"source ../test/utils.sh && query_graphql \
    \"query { \
        standingSellOrderBatches(where: { \
          accountId: \\\"0000000000000000000000000000000000000000\\\" , \
          batchIndex: 1, \
          validFromAuctionId: 2 \
        }) { \
        orders { \
            buyToken \
            sellToken \
            buyAmount \
            sellAmount \
        } \
      } \
    }\" | grep \"buyAmount.:.0.,.buyToken.:0,.sellAmount.:.0.,.sellToken.:0\""

step "Place matching sell order for standing order" \
"npx truffle exec scripts/snapp/sell_order.js --accountId=1 --buyToken=2 --sellToken=1 --minBuy=1 --maxSell=1"

step "Advance time to bid for auction" \
"npx truffle exec scripts/wait_seconds.js 181"

step_with_retry "Wait for bid to be placed" \
"npx truffle exec scripts/snapp/invokeViewFunction.js auctions 2 | grep \"solver: '0x90F8bf6A479f320ead074411a4B0e7944Ea8c9C1'\" "

step "Advance time to apply auction" \
"npx truffle exec scripts/wait_seconds.js 181"

step_with_retry "Standing Order was no longer traded" \
"source ../test/utils.sh && query_graphql \
    \"query { \
        accountStates(where: {stateIndex: \\\"4\\\"}) {\
            balances \
        } \
    }\" | jq .data.accountStates[0].balances[1] | grep -w -2 2000000000000000000"