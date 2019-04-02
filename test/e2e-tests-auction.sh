#!/bin/bash
set -e

cd dex-contracts/

# Place 6 orders in current Auction (accountId, buyToken, sellToken, minBuy, maxSell)
truffle exec scripts/sell_order.js 1 2 3 12 12
truffle exec scripts/sell_order.js 2 3 2 22 20
truffle exec scripts/sell_order.js 3 1 3 15 10
truffle exec scripts/sell_order.js 4 1 2 180 15
truffle exec scripts/sell_order.js 5 2 1 4 52
truffle exec scripts/sell_order.js 6 3 1 20 280

truffle exec scripts/mine_blocks.js 21
sleep 5

# Test Listener (order put into Database) - Query mongodb
# mongo --eval "db.dfusion.orders.find({'slot': 1})"
# TODO


# Wait for state update because of closed auction

# TODO

# Check that Auction Settlement is something different
# In this case the simple solver would match the two orders with agreeing limit prices between two tokens.

# TODO
