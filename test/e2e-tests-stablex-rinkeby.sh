#!/bin/bash

cd dex-contracts
source ../test/utils.sh

step "Deposit Fee Token " \
"npx truffle exec scripts/stablex/deposit.js --network=rinkeby --accountId=0 --tokenId=0 --amount=0.001"

step "Deposit Stablecoin " \
"npx truffle exec scripts/stablex/deposit.js --network=rinkeby --accountId=0 --tokenId=7 --amount=0.001"

step "Create Market Order Fee Token => Stablecoin " \
"npx truffle exec scripts/stablex/place_order.js --network=rinkeby --accountId=0 --buyToken=7 --sellToken=0 --minBuy=0.1 --maxSell=1 --validFor=2"

step "Create Market Order Stablecoin => Fee Token " \
"npx truffle exec scripts/stablex/place_order.js --network=rinkeby --accountId=0 --buyToken=0 --sellToken=7 --minBuy=0.1 --maxSell=1 --validFor=1"

time_remaining_hex=`npx truffle exec scripts/stablex/invokeViewFunction.js --network rinkeby getSecondsRemainingInBatch | grep -Eo '[0-9]+'`
time_to_sleep=`echo $((16#$time_remaining_hex + 30))`

echo "Will sleep for $time_to_sleep seconds"

step "Wait for batch to be closed and solved (+30 seconds) " \
"sleep $time_to_sleep"

step "Check there are no errors in the solver logs " \
"! docker-compose logs | grep -e ERROR"