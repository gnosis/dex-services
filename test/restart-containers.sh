#!/bin/bash
set -e

docker-compose rm -sf mongo
docker-compose up -d mongo
docker-compose rm -sf ganache-cli
docker-compose up -d ganache-cli
docker-compose restart listener
cd dex-contracts && wait-port -t 30000 8545 && truffle migrate && cd -