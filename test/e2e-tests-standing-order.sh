#!/bin/bash
set -e

cd dex-contracts/

truffle exec scripts/sell_order.js 1 2 1 1 1

# Place standing order in current Auction (accountId, buyToken, sellToken, minBuy, maxSell)
truffle exec scripts/standing_order.js 0 1 2 1 1

# Check standing order has been recorded
retry -t 5 "mongo dfusion2 --eval \"db.standing_orders.findOne({accountId:0, batchIndex:1, orders: [ { buyToken:1, sellToken:2, buyAmount:'1000000000000000000', sellAmount:'1000000000000000000' }]})\" | grep ObjectId"

truffle exec scripts/wait_seconds.js 181

# Assert Standing order account traded
retry -t 5 "mongo dfusion2 --eval \"db.accounts.findOne({'stateIndex': 5}).balances[1]\" | grep -2 1000000000000000000"

# Next Batch: Make sure standing order is still active and trading
truffle exec scripts/sell_order.js 1 2 1 1 1
truffle exec scripts/wait_seconds.js 181
retry -t 5 "mongo dfusion2 --eval \"db.accounts.findOne({'stateIndex': 6}).balances[1]\" | grep -2 2000000000000000000"

# Update, then cancel standing order in same batch (make sure only cancel gets processed)
truffle exec scripts/standing_order.js 0 1 2 1 2
truffle exec scripts/standing_order.js 0 0 0 0 0

# Make sure it's no longer traded
truffle exec scripts/sell_order.js 1 2 1 1 1
truffle exec scripts/wait_seconds.js 181
retry -t 5 "mongo dfusion2 --eval \"db.accounts.findOne({'stateIndex': 7}).balances[1]\" | grep -2 2000000000000000000"