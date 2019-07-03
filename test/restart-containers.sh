#!/bin/bash
set -e

docker-compose kill mongo
docker-compose rm -f mongo
docker-compose up -d mongo
docker-compose kill ganache-cli 
docker-compose rm -f ganache-cli
docker-compose up -d ganache-cli
docker-compose restart listener
cd dex-contracts && retry truffle migrate && cd -