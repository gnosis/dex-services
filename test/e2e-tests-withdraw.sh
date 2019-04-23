#!/bin/bash
set -e

cd dex-contracts/

truffle exec scripts/request_withdraw.js 3 3 18
truffle exec scripts/mine_blocks.js 21

sleep 5
truffle exec scripts/invokeViewFunction.js 'getCurrentStateRoot'

EXPECTED_HASH="c6d77fdc8145d79170386ab39fb059d40827b2ed0b891902b678fcd5294c91b6"
truffle exec scripts/invokeViewFunction.js 'getCurrentStateRoot' | grep ${EXPECTED_HASH}
mongo dfusion2 --eval "db.accounts.findOne({'stateHash': '$EXPECTED_HASH'}).balances[62]" | grep 0

truffle exec scripts/claim_withdraw.js 1 3 3 | grep "Success! Balance of token 3 before claim: 2682000000000000000000, after claim: 2700000000000000000000"