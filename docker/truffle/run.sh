#!/bin/sh

node_modules/.bin/truffle migrate --reset --network developmentdocker
touch build/migration.flag