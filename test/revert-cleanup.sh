#!/bin/bash

echo "Revert Ganache to Pre-Setup"
cd dex-contracts
truffle exec scripts/ganache/revertSnapshot.js $1

echo "Dropping Deposits"
mongo dfusion2 --eval "db.deposits.drop()" | grep -w true

echo "Dropping Withdraws"
mongo dfusion2 --eval "db.withdraws.drop()" | grep -w true

echo "Dropping Orders"
mongo dfusion2 --eval "db.orders.drop()" | grep -w true

echo "Dropping Standing Orders"
mongo dfusion2 --eval "db.standing_orders.drop()" | grep -w true


echo "Remove non-trivial accounts"
mongo dfusion2 --eval "db.accounts.remove({ 'stateIndex': { \$gt: 0 } })"

echo "Great success!"

