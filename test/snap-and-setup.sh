#!/bin/bash

cd dex-contracts/

SNAP_ID=$(truffle exec scripts/ganache/makeSnapshot.js | grep '0x')
echo "Snapshot ID - ${SNAP_ID}"

truffle exec scripts/setup_environment.js 6