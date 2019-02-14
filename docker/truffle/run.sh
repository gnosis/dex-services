#!/bin/bash

cd dex-contracts

# Trick to run npm install in a git submodule
rm -fr .git || :

# Compile necesary contracts for app and cleanup unnecesary files
npm install

# # Compile necesary contracts for app and cleanup unnecesary files

# truffle compile
npx truffle migrate --reset --network developmentdocker

# running the actual tests
sh ./../test/e2e-tests.sh