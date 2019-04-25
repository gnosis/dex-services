#!/bin/bash
set -e

cd dex-contracts/

truffle exec scripts/request_withdraw.js 3 3 18
truffle exec scripts/wait_seconds.js 181

sleep 5
truffle exec scripts/invokeViewFunction.js 'getCurrentStateRoot'

EXPECTED_HASH="77b01abfbad57cb7a1344b12709603ea3b9ad803ef5ea09814ca212748f54733"
truffle exec scripts/invokeViewFunction.js 'getCurrentStateRoot' | grep ${EXPECTED_HASH}
mongo dfusion2 --eval "db.accounts.findOne({'stateHash': '$EXPECTED_HASH'}).balances[62]" | grep 0

truffle exec scripts/claim_withdraw.js 0 3 3 | grep "Success! Balance of token 3 before claim: 282000000000000000000, after claim: 300000000000000000000"