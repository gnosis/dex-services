#!/bin/bash
set -e

docker-compose rm -sf
docker-compose up -d
cd dex-contracts && wait-port -t 30000 8545 && truffle migrate && cd -
