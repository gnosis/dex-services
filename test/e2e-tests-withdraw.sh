#!/usr/bin/bash
set -e

cd dex-contracts/

truffle exec scripts/request_withdraw.js 3 3 18
truffle exec scripts/mine_blocks.js 21

#TODO @josojo: this should happen by the driver
#truffle exec scripts/apply_withdrawals.js 0 0x0000000000000000000000000000000000000000000000000000000000000001 0x0000000000000000000000000000000000000000000000000000000000000001
#truffle exec scripts/apply_withdrawals.js 1 0x0000000000000000000000000000000000000000000000000000000000000002 0x0000000000000000000000000000000000000000000000000000000000000002
sleep 5
truffle exec scripts/invokeViewFunction.js 'getCurrentStateRoot'

EXPECTED_HASH="0000000000000000000000000000000000000000000000000000000000000002"
truffle exec scripts/invokeViewFunction.js 'getCurrentStateRoot' | grep $EXPECTED_HASH
mongo dfusion2 --eval "db.accounts.findOne({'stateHash': '$EXPECTED_HASH'}).balances[62]" | grep 0
