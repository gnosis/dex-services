#!/bin/bash
set -e

cd dex-contracts/

EXPECTED_AUCTION=0

# Ensure no orders yet in auction slot 1
mongo dfusion2 --eval "db.orders.find({'auctionId': ${EXPECTED_AUCTION}}).size()" | grep 0

# Make sure we have enough balances for the trades
truffle exec scripts/deposit.js 1 3 300
truffle exec scripts/deposit.js 2 2 300
truffle exec scripts/deposit.js 3 3 200
truffle exec scripts/deposit.js 4 2 300
truffle exec scripts/deposit.js 5 1 300
truffle exec scripts/deposit.js 6 1 300

# Place 6 orders in current Auction (accountId, buyToken, sellToken, minBuy, maxSell)
truffle exec scripts/sell_order.js 1 2 3 12 12
truffle exec scripts/sell_order.js 2 3 2 2.2 2
truffle exec scripts/sell_order.js 3 1 3 150 10
truffle exec scripts/sell_order.js 4 1 2 180 15
truffle exec scripts/sell_order.js 5 2 1 4 52
truffle exec scripts/sell_order.js 6 3 1 20 280

sleep 5

# Test Listener: There are now 6 orders in auction slot 1 and sellAmount for accountId = 6 is 280000000000000000000
mongo dfusion2 --eval "db.orders.find({'auctionId': ${EXPECTED_AUCTION}}).size()" | grep 6
mongo dfusion2 --eval "db.orders.findOne({'auctionId': ${EXPECTED_AUCTION}, 'accountId': 6}).sellAmount" | grep 280000000000000000000

truffle exec scripts/mine_blocks.js 21

sleep 10

# Test balances have been updated
EXPECTED_HASH="c4c44a0c0c17022dc987ba8abbc89d0c77d20865d0d61c07f76c889badd708a2"
truffle exec scripts/invokeViewFunction.js 'getCurrentStateRoot' | grep ${EXPECTED_HASH}
mongo dfusion2 --eval "db.accounts.findOne({'stateHash': '$EXPECTED_HASH'}).balances[60]" | grep 4000000000000000000
