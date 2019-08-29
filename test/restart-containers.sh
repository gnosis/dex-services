#!/bin/bash
set -e

docker-compose kill
docker-compose rm -sf postgres
docker-compose up -d driver graph-listener

cd dex-contracts && npx wait-port -t 30000 8545 && npx truffle migrate && cd -
