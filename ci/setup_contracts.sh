#!/usr/bin/env bash

set -e

docker-compose up -d ganache-cli
(cd contracts; cargo run --locked --bin deploy --features bin)
