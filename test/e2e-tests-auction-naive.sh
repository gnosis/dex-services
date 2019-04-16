#!/bin/bash
set -e

cd dex-contracts/

EXPECTED_AUCTION=1


truffle exec scripts/setup_environment.js

# checks state after first new deposit round with a deposit of 18 from account 3 and token 3
truffle exec scripts/deposit.js 1 1 10 
truffle exec scripts/deposit.js 2 2 10 
truffle exec scripts/mine_blocks.js 21

# Ensure no orders yet in auction slot 1
mongo dfusion2 --eval "db.orders.find({'auctionId': ${EXPECTED_AUCTION}}).size()" | grep 0

# Place 6 orders in current Auction (accountId, buyToken, sellToken, minBuy, maxSell)
truffle exec scripts/sell_order.js 1 2 1 10 10
truffle exec scripts/sell_order.js 2 1 2 10 10

sleep 5

truffle exec scripts/mine_blocks.js 21

# TODO - Test Driver: Wait for state update because of closed auction

# TODO - Test Solver: Ensure that balances are differ after state transition (i.e. found non-trivial solution)