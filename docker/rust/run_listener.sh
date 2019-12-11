#!/bin/bash
set -e

# Allow ganache to be started and listening
timeout 5 bash -c 'until echo > /dev/tcp/ganache-cli/8545 ; do sleep 0.5; done' 2>/dev/null

# Make sure ganache has at least one block mined so indexing can start
curl --silent -H "Content-Type: application/json" -X POST --data \
        '{"id":1337,"jsonrpc":"2.0","method":"evm_mine","params":[]}' \
        http://ganache-cli:8545

cargo run --release --bin listener
