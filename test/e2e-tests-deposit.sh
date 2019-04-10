#!/bin/bash
set -e

cd dex-contracts/

truffle exec scripts/setup_environment.js

# checks state after first new deposit round with a deposit of 18 from account 3 and token 3
truffle exec scripts/deposit.js 3 3 18 
truffle exec scripts/mine_blocks.js 21

sleep 5

EXPECTED_HASH="73899d50b4ec5e351b4967e4c4e4a725e0fa3e8ab82d1bb6d3197f22e65f0c97"
truffle exec scripts/invokeViewFunction.js 'getCurrentStateRoot' | grep $EXPECTED_HASH
mongo dfusion2 --eval "db.accounts.findOne({'stateHash': '$EXPECTED_HASH'}).balances[62]" | grep 18