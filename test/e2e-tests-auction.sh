#!/bin/bash
set -e

cd dex-contracts
truffle exec scripts/setup_environment.js 6

EXPECTED_AUCTION=0

# Ensure no orders yet in auction slot 1
mongo dfusion2 --eval "db.orders.find({'auctionId': ${EXPECTED_AUCTION}}).size()" | grep -w 0

# Make sure we have enough balances for the trades
truffle exec scripts/deposit.js 0 2 300
truffle exec scripts/deposit.js 1 1 300
truffle exec scripts/deposit.js 2 2 200
truffle exec scripts/deposit.js 3 1 300
truffle exec scripts/deposit.js 4 0 300
truffle exec scripts/deposit.js 5 0 300

# Place 6 orders in current Auction (accountId, buyToken, sellToken, minBuy, maxSell)
truffle exec scripts/sell_order.js 0 1 2 12 12
truffle exec scripts/sell_order.js 1 2 1 2.2 2
truffle exec scripts/sell_order.js 2 0 2 150 10
truffle exec scripts/sell_order.js 3 0 1 180 15 # executed sell: 4
truffle exec scripts/sell_order.js 4 1 0 4 52 # executed sell: 52
truffle exec scripts/sell_order.js 5 2 0 20 280

# Test Listener: There are now 6 orders in auction slot 1 and sellAmount for accountId = 5 is 280000000000000000000
retry -t 5 "mongo dfusion2 --eval \"db.orders.find({'auctionId': ${EXPECTED_AUCTION}}).size()\" | grep -w 6"
retry -t 5 "mongo dfusion2 --eval \"db.orders.findOne({'auctionId': ${EXPECTED_AUCTION}, 'accountId': 5}).sellAmount\" | grep -w 280000000000000000000"

truffle exec scripts/wait_seconds.js 181

# Test balances have been updated
EXPECTED_HASH="2b87dc830d051be72f4adcc3677daadab2f3f2253e9da51d803faeb0daa1532f"
retry -t 5 "truffle exec scripts/invokeViewFunction.js 'getCurrentStateRoot' | grep ${EXPECTED_HASH}"

# Account 4 has now 4 of token 1 
retry -t 5 "mongo dfusion2 --eval \"db.accounts.findOne({'stateHash': '$EXPECTED_HASH'}).balances[121]\" | grep -w 4000000000000000000"

# Account 3 has now 52 of token 0
retry -t 5 "mongo dfusion2 --eval \"db.accounts.findOne({'stateHash': '$EXPECTED_HASH'}).balances[90]\" | grep -2 52000000000000000000"
