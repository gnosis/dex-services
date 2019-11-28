#!/bin/bash
set -e

docker-compose up -d ganache-cli
cd dex-contracts
npm ci
npx wait-port -t 30000 8545
npx truffle migrate