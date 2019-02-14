#!/usr/bin/env bats

# checks state after first new deposit round with a deposit of 18 from account 3 and token 3
npx truffle exec scripts/setup_environment.js --network developmentdocker
npx truffle exec --network developmentdocker  scripts/deposit.js 3 3 18 
npx truffle exec --network developmentdocker scripts/mine_blocks.js 21 
sleep 1s
npx truffle exec --network developmentdocker scripts/invokeViewFunction.js 'getCurrentStateRoot' | grep "0x" > result.txt
npx truffle exec --network developmentdocker scripts/invokeViewFunction.js 'getCurrentStateRoot' | grep 0x77b01abfbad57cb7a1344b12709603ea3b9ad803ef5ea09814ca212748f54733
