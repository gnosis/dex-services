#!/bin/bash

cd dex-contracts
source ../test/utils.sh

step "Setup" \
"truffle exec scripts/setup_environment.js 6"

###############
# Deposit Tests
###############

step "Deposit 18 of token 2 for user 2" \
"truffle exec scripts/deposit.js 2 2 18"

step_with_retry "Deposit was added to graph db" \
"source ../test/utils.sh && query_graphql \
    \"query { \
        deposits(where: { accountId: 2}) { \
            amount \
        } \
    }\" | grep 18000000000000000000"

step "Advance time to finalize batch" \
"truffle exec scripts/wait_seconds.js 181"

EXPECTED_HASH="73815c173218e6025f7cb12d0add44354c4671e261a34a360943007ff6ac7af5"

step_with_retry "Check contract updated" \
"truffle exec scripts/invokeViewFunction.js 'getCurrentStateRoot' | grep ${EXPECTED_HASH}"

step_with_retry "Check mongo db updated" \
"mongo dfusion2 --eval \"db.accounts.findOne({'stateHash': '${EXPECTED_HASH}'}).balances[62]\" | grep -w 18000000000000000000"

step_with_retry "Check graph db updated" \
"source ../test/utils.sh && query_graphql \
    \"query { \
        accountStates(where: {id: \\\"${EXPECTED_HASH}\\\"}) {\
            balances \
        } \
    }\" | jq .data.accountStates[0].balances[62] | grep -w 18000000000000000000"

################
# Withdraw Tests
################

step "Request withdraw of 18 of token 2 by account 2" \
    "truffle exec scripts/request_withdraw.js 2 2 18"

step_with_retry "Withdraw was added to graph db" \
"source ../test/utils.sh && query_graphql \
    \"query { \
        withdraws(where: { accountId: 2 }) { \
            amount \
        } \
    }\" | grep 18000000000000000000"

step "wait till withdraw slot becomes inactive" "truffle exec scripts/wait_seconds.js 181"

EXPECTED_HASH="7b738197bfe79b6d394499b0cac0186cdc2f65ae2239f2e9e3c698709c80cb67"

step_with_retry "Check contract updated" \
"truffle exec scripts/invokeViewFunction.js 'getCurrentStateRoot' | grep ${EXPECTED_HASH}"

step_with_retry "Check mongo db updated" \
 "mongo dfusion2 --eval \"db.accounts.findOne({'stateHash': '$EXPECTED_HASH'}).balances[62]\" | grep -w 0"

# Should now be able to claim withdraw and see a balance change
step "Claim Withdraw" \
"truffle exec scripts/claim_withdraw.js 0 2 2 | grep \"Success! Balance of token 2 before claim: 282000000000000000000, after claim: 300000000000000000000\""

