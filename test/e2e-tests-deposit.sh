#!/bin/bash
set -e

cd dex-contracts/

truffle exec scripts/setup_environment.js 6

# checks state after first new deposit round with a deposit of 18 from account 2 and token 2
truffle exec scripts/deposit.js 2 2 18

truffle exec scripts/mine_blocks.js 21

sleep 10

EXPECTED_HASH="a5b2329a51ada3ce2114e2724264cbfd11f5cd63e41c3700c3f88358995b6153"
truffle exec scripts/invokeViewFunction.js 'getCurrentStateRoot' | grep ${EXPECTED_HASH}
mongo dfusion2 --eval "db.accounts.findOne({'stateHash': '${EXPECTED_HASH}'}).balances[62]" | grep 18000000000000000000