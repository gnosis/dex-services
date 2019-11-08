#!/bin/bash

cd dex-contracts
source ../test/utils.sh

step "Setup" \
"npx truffle exec scripts/stablex/setup_environment.js"

step "Make sure we have enough balances for the trades" \
"npx truffle exec scripts/stablex/deposit.js --accountId=0 --tokenId=0 --amount=3000 && \
npx truffle exec scripts/stablex/deposit.js --accountId=1 --tokenId=1 --amount=3000"

step "Place 2 orders in current auction" \
"npx truffle exec scripts/stablex/place_order.js --accountId=0 --buyToken=1 --sellToken=0 --minBuy=999 --maxSell=2000 --validFor=20 && \
npx truffle exec scripts/stablex/place_order.js --accountId=1 --buyToken=0 --sellToken=1 --minBuy=1996 --maxSell=999 --validFor=20 "

step "Advance time to close auction" \
"npx truffle exec scripts/wait_seconds.js 300"

step "Request withdraw" \
"npx truffle exec scripts/stablex/request_withdraw.js --accountId=0 --tokenId=1 --amount=999"

step "Advance time to finalize withdraw" \
"npx truffle exec scripts/wait_seconds.js 300"

npx truffle exec scripts/stablex/claim_withdraw.js --accountId=0 --tokenId=1