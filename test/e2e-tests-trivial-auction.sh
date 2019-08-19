#!/bin/bash

cd dex-contracts
source ../test/utils.sh

step "Setup" \
"npx truffle exec scripts/setup_environment.js"

EXPECTED_AUCTION=0

step "Ensure no orders yet in auction slot 1" \
"mongo dfusion2 --eval \"db.orders.find({'auctionId': ${EXPECTED_AUCTION}}).size()\" | grep -w 0"

step "Place 2 orders that cannot be filled" \
"npx truffle exec scripts/sell_order.js 0 0 1 1 1 && \
 npx truffle exec scripts/sell_order.js 1 1 2 1 1"

step "Advance time to apply auction" \
"npx truffle exec scripts/wait_seconds.js 181"

EXPECTED_HASH="b68a9937fc35e1315fda593caa4a171356de46ac8649f7147c5923ce5faf2a60"
step_with_retry "Test balances have not been updated" \
"npx truffle exec scripts/invokeViewFunction.js 'getCurrentStateRoot' | grep ${EXPECTED_HASH}"
