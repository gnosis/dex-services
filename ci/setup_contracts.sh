#!/usr/bin/env bash

set -e

docker-compose up -d ganache-cli
(cd contracts/deploy; cargo run)
