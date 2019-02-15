#!/usr/bin/env bats
cd dex-contracts/
truffle migrate --compile-all

# checks state after first new deposit round with a deposit of 18 from account 3 and token 3
truffle exec scripts/setup_environment.js
truffle exec scripts/deposit.js 3 3 18 
truffle exec scripts/mine_blocks.js 21 
sleep 1s
truffle exec scripts/invokeViewFunction.js 'getCurrentStateRoot'
