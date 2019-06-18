#!/bin/bash
set -e

cd dex-contracts/

# Place standing order in current Auction (accountId, buyToken, sellToken, minBuy, maxSell)
truffle exec scripts/standing_order.js 0 1 2 12 12

# Check standing order has been recorded
retry -t 5 "mongo dfusion2 --eval \"db.standing_orders.findOne({accountId:0, batchIndex:1, orders: [ { buyToken:1, sellToken:2, buyAmount:'12000000000000000000', sellAmount:'12000000000000000000' }]})\" | grep ObjectId"