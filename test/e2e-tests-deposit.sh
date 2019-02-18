#!/usr/bin/env bats
set -e

cd dex-contracts/

# checks state after first new deposit round with a deposit of 18 from account 3 and token 3
truffle exec scripts/setup_environment.js
truffle exec scripts/deposit.js 3 3 18 
truffle exec scripts/mine_blocks.js 21 
sleep 5s

EXPECTED_HASH="f1940d119100aae087a3a8f202a23a8b81486908576cb63eb6261fdc72e23b67"
truffle exec scripts/invokeViewFunction.js 'getCurrentStateRoot' | grep $EXPECTED_HASH
mongo dfusion2 --eval "db.accounts.findOne({'stateHash': '$EXPECTED_HASH'}).balances[62]" | grep 18