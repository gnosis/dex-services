#!/bin/bash
set -e

cd dex-contracts/

truffle exec scripts/setup_environment.js 6

###############
# Deposit Tests
###############

# checks state after first new deposit round with a deposit of 18 from account 2 and token 2
truffle exec scripts/deposit.js 2 2 18

# Wait till previous deposit slot becomes inactive
truffle exec scripts/wait_seconds.js 181

sleep 10

# Expect that driver has processed deposit slot and ensure updated balances are as expected
EXPECTED_HASH="a5b2329a51ada3ce2114e2724264cbfd11f5cd63e41c3700c3f88358995b6153"
truffle exec scripts/invokeViewFunction.js 'getCurrentStateRoot' | grep ${EXPECTED_HASH}
mongo dfusion2 --eval "db.accounts.findOne({'stateHash': '${EXPECTED_HASH}'}).balances[62]" | grep 18000000000000000000

################
# Withdraw Tests
################

# Request withdraw of 18 of token 2 by account 2 and wait till withdraw slot becomes inactive.
truffle exec scripts/request_withdraw.js 2 2 18
truffle exec scripts/wait_seconds.js 181

sleep 5
# Expect that driver has processed withdraw slot and ensure updated balances are as expected
EXPECTED_HASH="77b01abfbad57cb7a1344b12709603ea3b9ad803ef5ea09814ca212748f54733"
truffle exec scripts/invokeViewFunction.js 'getCurrentStateRoot' | grep ${EXPECTED_HASH}
mongo dfusion2 --eval "db.accounts.findOne({'stateHash': '$EXPECTED_HASH'}).balances[62]" | grep 0

# Should now be able to claim withdraw and see a balance change
truffle exec scripts/claim_withdraw.js 0 2 2 | grep "Success! Balance of token 2 before claim: 282000000000000000000, after claim: 300000000000000000000"