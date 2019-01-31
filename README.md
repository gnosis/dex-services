Intro
============
This repository contains the backend logic for the dfusion exchange based on the specification, see [here](github.com/gnosis/dex-research)


Instructinos
============


Clone the repository, its submodule, and run the container
```bash
git clone git@github.com:gnosis/dex-services.git
cd dex-services
git submodule init
git submodule update
docker-compose up
```

This will start:
ganche-cli, the local ethereum chain
mongodb, the data base storing the data of the snapp
listener, a listner pulling data from the ganache-cli and inserting it into mongodb
driver, a service calculating the new states and push these into the smart contract


After the docker is up an running, we have to migrate the smart contracts into ganache-cli

```bash
git clone git@github.com:gnosis/dex-contracts.git
cd dex-contracts
truffle migrate --reset
```

In order to setup some testing accounts and make the first deposits (from account 3, of the thrid registered token with an amount of 18), run in the same repo the following scripts:


```bash
truffle exec scripts/setup_environment.js
truffle exec scripts/deposit.js 3 3 18
truffle exec scripts/mineBlocks.js 21
```
