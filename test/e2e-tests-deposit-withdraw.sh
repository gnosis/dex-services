#!/bin/bash
set -e

# Make sure jq is installed (retry doesn't give a good error message at the moment)
jq --version

cd dex-contracts
truffle exec scripts/setup_environment.js 6

###############
# Deposit Tests
###############

# checks state after first new deposit round with a deposit of 18 from account 2 and token 2
truffle exec scripts/deposit.js 2 2 18

# check that deposit was added to the database
retry -t 5 "source ../test/utils.sh && query_graphql \
    'query { \
        deposits(where: { accountId: 2}) { \
            amount \
        } \
    }' | grep 18000000000000000000"

# Wait till previous deposit slot becomes inactive
truffle exec scripts/wait_seconds.js 181

# Expect that driver has processed deposit slot and ensure updated balances are as expected
EXPECTED_HASH="73815c173218e6025f7cb12d0add44354c4671e261a34a360943007ff6ac7af5"
retry -t 5 "truffle exec scripts/invokeViewFunction.js 'getCurrentStateRoot' | grep ${EXPECTED_HASH}"
retry -t 5 "mongo dfusion2 --eval \"db.accounts.findOne({'stateHash': '${EXPECTED_HASH}'}).balances[62]\" | grep -w 18000000000000000000"
retry -t 5 "source ../test/utils.sh && query_graphql \
    'query { \
        accountStates(where: {id: \\\"${EXPECTED_HASH}\\\"}) {\
            balances \
        } \
    }' | jq .data.accountStates[0].balances[62] | grep -w 18000000000000000000"

################
# Withdraw Tests
################

# Request withdraw of 18 of token 2 by account 2 and wait till withdraw slot becomes inactive.
truffle exec scripts/request_withdraw.js 2 2 18

# check that withdraw was added to the database
retry -t 5 "source ../test/utils.sh && query_graphql \
    'query { \
        withdraws(where: { accountId: 2}) { \
            amount \
        } \
    }' | grep 18000000000000000000"

truffle exec scripts/wait_seconds.js 181

# Expect that driver has processed withdraw slot and ensure updated balances are as expected
EXPECTED_HASH="7b738197bfe79b6d394499b0cac0186cdc2f65ae2239f2e9e3c698709c80cb67"
retry -t 5 "truffle exec scripts/invokeViewFunction.js 'getCurrentStateRoot' | grep ${EXPECTED_HASH}"
retry -t 5 "mongo dfusion2 --eval \"db.accounts.findOne({'stateHash': '$EXPECTED_HASH'}).balances[62]\" | grep -w 0"

# Should now be able to claim withdraw and see a balance change
truffle exec scripts/claim_withdraw.js 0 2 2 | grep "Success! Balance of token 2 before claim: 282000000000000000000, after claim: 300000000000000000000"
