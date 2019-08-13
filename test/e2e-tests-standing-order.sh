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

step_with_retry "Check mongo standing order has been recorded" \
"mongo dfusion2 --eval \"db.standing_orders.findOne({accountId:0, batchIndex:0, validFromAuctionId:0, orders: [ { buyToken:1, sellToken:2, buyAmount:'1000000000000000000', sellAmount:'1000000000000000000' }]})\" | grep ObjectId"

step_with_retry "Check graph standing order batch has been recorded" \
"source ../test/utils.sh && query_graphql \
    \"query { \
        standingSellOrderBatches(where: { \
          accountId: 0, \
          batchIndex: 0, \
          validFromAuctionId: 0 \
        }) { \
        orders { sellAmount } \
      } \
    }\" | grep 1000000000000000000"

step_with_retry "Check graph standing order has been recorded" \
"source ../test/utils.sh && query_graphql \
    \"query { \
        sellOrders (where: { \
          accountId: 0, \
          auctionId: null, \
          slotIndex: null \
          buyToken: 2, \
          sellToken: 1, \
          buyAmount: \"1000000000000000000\", \
          sellAmount: \"1000000000000000000\" \
        }) { \
          buyAmount \
        } \
    }\" | grep 1000000000000000000"

step "Advance time to apply auction" \
"npx truffle exec scripts/wait_seconds.js 181"

step_with_retry "Assert Standing order account traded" \
"mongo dfusion2 --eval \"db.accounts.findOne({'stateIndex': 2}).balances[1]\" | grep -2 1000000000000000000"

step "Place matching sell order for standing order" \
"npx truffle exec scripts/sell_order.js 1 2 1 1 1"

step "Advance time to apply auction" \
"npx truffle exec scripts/wait_seconds.js 181"

step_with_retry "Make sure standing order is still traded" \
"mongo dfusion2 --eval \"db.accounts.findOne({'stateIndex': 3}).balances[1]\" | grep -2 2000000000000000000"

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
        orders { sellAmount } \
      } \
    }\" | grep 2000000000000000000"

step_with_retry "Check graph standing order has been updated" \
"source ../test/utils.sh && query_graphql \
    \"query { \
        sellOrders (where: { \
          accountId: 0, \
          auctionId: null, \
          slotIndex: null, \
          buyToken: 2, \
          sellToken: 1, \
          buyAmount: \"1000000000000000000\", \
          sellAmount: \"2000000000000000000\" \
        }) { \
          buyAmount \
        } \
    }\" | grep 1000000000000000000"

step "Cancel standing order in same batch (make sure only cancel gets processed)" \
"npx truffle exec scripts/standing_order.js 0 0 0 0 0"

# TODO: Review after the PR that discard orders with sellVolume 0 
#      (in that case, here we have to test that there's no order for the batch of the use)
step_with_retry "Check graph standing order batch has been deleted" \
"source ../test/utils.sh && query_graphql \
    \"query { \
        standingSellOrderBatches(where: { \
          accountId: 0, \
          batchIndex: 1, \
          validFromAuctionId: 2 \
        }) { \
        orders { sellAmount } \
      } \
    }\" | grep 0"

step_with_retry "Check graph standing order has been deleted" \
"source ../test/utils.sh && query_graphql \
    \"query { \
        sellOrders (where: { \
          accountId: 0, \
          auctionId: null, \
          slotIndex: null, \
          buyToken: 0, \
          sellToken: 0, \
          buyAmount: 0, \
          sellAmount: 0 \
        }) { \
          buyAmount \
        } \
    }\" | grep 0"

step "Place matching sell order for standing order" \
"npx truffle exec scripts/sell_order.js 1 2 1 1 1"

step "Advance time to apply auction" \
"npx truffle exec scripts/wait_seconds.js 181"

step_with_retry "Standing Order was no longer traded" \
"mongo dfusion2 --eval \"db.accounts.findOne({'stateIndex': 4}).balances[1]\" | grep -2 2000000000000000000"