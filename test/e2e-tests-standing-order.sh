#!/bin/bash

cd dex-contracts
source ../test/utils.sh

step "Setup" \
"npx truffle exec scripts/setup_environment.js 6"

step "Ensure sufficient Balances" \
"npx truffle exec scripts/deposit.js 0 2 300 && \
 npx truffle exec scripts/deposit.js 1 1 300 && \
 npx truffle exec scripts/wait_seconds.js 181"

step "Place Sell Order" \
"npx truffle exec scripts/sell_order.js 1 2 1 1 1"

step "Place standing order in current Auction" \
"npx truffle exec scripts/standing_order.js 0 1 2 1 1"

step_with_retry "Check graph standing order batch has been recorded" \
"source ../test/utils.sh && query_graphql \
    \"query { \
        standingSellOrderBatches(where: { \
          accountId: 0, \
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

step "Advance time to apply auction" \
"npx truffle exec scripts/wait_seconds.js 181"

step "Place matching sell order for standing order" \
"npx truffle exec scripts/sell_order.js 1 2 1 1 1"

step "Advance time to apply auction" \
"npx truffle exec scripts/wait_seconds.js 181"

step "Update standing order" \
"npx truffle exec scripts/standing_order.js 0 1 2 1 2"

step_with_retry "Check graph standing order batch has been updated" \
"source ../test/utils.sh && query_graphql \
    \"query { \
        standingSellOrderBatches(where: { \
          accountId: 0, \
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
"npx truffle exec scripts/standing_order.js 0 0 0 0 0"

step_with_retry "Check graph standing order batch has been deleted" \
"source ../test/utils.sh && query_graphql \
    \"query { \
        standingSellOrderBatches(where: { \
          accountId: 0, \
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
"npx truffle exec scripts/sell_order.js 1 2 1 1 1"

step "Advance time to apply auction" \
"npx truffle exec scripts/wait_seconds.js 181"
