#!/bin/sh

export GANACHE_HOST='ganache-cli'
npx truffle migrate --reset
touch build/migration.flag