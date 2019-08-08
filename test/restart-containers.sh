#!/bin/bash
set -e


docker-compose rm -sf mongo
docker-compose up -d mongo

docker-compose rm -sf postgres
docker-compose up -d postgres

docker-compose rm -sf ganache-cli
docker-compose up -d ganache-cli

docker-compose restart listener
docker-compose restart graph-listener

cd dex-contracts && npx wait-port -t 30000 8545 && npx truffle migrate && cd -
