#!/usr/bin/env bats
echo $PWD

npx truffle exec scripts/setup_environment.js --network developmentdocker
npx truffle exec --network developmentdocker  scripts/deposit.js 3 3 18 
npx truffle exec --network developmentdocker scripts/mine_blocks.js 21 
sleep 1s
npx truffle exec --network developmentdocker scripts/invokeViewFunction.js 'getCurrentStateRoot' | grep "0x"> result.txt
diff result.txt ./../test/result_e2e_1.txt