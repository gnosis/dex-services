#!/usr/bin/env bats
cd dex-contracts/

truffle migrate
# checks state after first new deposit round with a deposit of 18 from account 3 and token 3
truffle exec scripts/setup_environment.js
truffle exec scripts/deposit.js 3 3 18 
truffle exec scripts/mine_blocks.js 21 
sleep 1s
truffle exec scripts/invokeViewFunction.js 'getCurrentStateRoot' | grep 0x77b01abfbad57cb7a1344b12709603ea3b9ad803ef5ea09814ca212748f54733
