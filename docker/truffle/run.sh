#!/bin/bash

cd dex-contracts

rm -fr .git || :
# # Compile necesary contracts for app and cleanup unnecesary files
npm install

# # Compile necesary contracts for app and cleanup unnecesary files
# truffle compile
npx truffle migrate --reset --network developmentdocker