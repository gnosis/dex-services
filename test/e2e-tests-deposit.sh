#!/bin/bash
set -e

cd dex-contracts/

truffle exec scripts/setup_environment.js

# checks state after first new deposit round with a deposit of 18 from account 3 and token 3
truffle exec scripts/deposit.js 3 3 18 

truffle exec scripts/deposit.js 1 3 300
truffle exec scripts/deposit.js 2 2 300
truffle exec scripts/deposit.js 3 3 300
truffle exec scripts/deposit.js 4 2 300
truffle exec scripts/deposit.js 5 1 300
truffle exec scripts/deposit.js 6 1 300

truffle exec scripts/mine_blocks.js 21

sleep 10

EXPECTED_HASH="6af094aed402d5fd6427d28989b7621e4a2558c88a00d8a21c5d741b79da874e"
truffle exec scripts/invokeViewFunction.js 'getCurrentStateRoot' | grep ${EXPECTED_HASH}
mongo dfusion2 --eval "db.accounts.findOne({'stateHash': '${EXPECTED_HASH}'}).balances[62]" | grep 18000000000000000000