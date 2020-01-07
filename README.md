[![Build Status](https://travis-ci.com/gnosis/dex-services.svg?branch=master)](https://travis-ci.com/gnosis/dex-services)

## Intro

This repository contains the backend logic for the dfusion exchange based on [this specification](github.com/gnosis/dex-research).

It contains two sub-projects that both implement the market mechanism described above in different ways. An fully on-chain solution with instant finality but limited scalability (referred to as "StableX") and a preliminary version that intends to achieves scalability by offloading computation and data-storage off-chain using an [optimistic roll-up](https://medium.com/plasma-group/ethereum-smart-contracts-in-l2-optimistic-rollup-2c1cef2ec537) approach. The latter is in early development stage and not yet ready for use.

## Getting Started

### Requirements

- Rust (stable)
- NodeJS 11.0, starting with version 12 some deprecated APIs were removed that cause `scrypt`, `keccak`, `secp256k1`, and `sha3` packages to fail to build
- Docker and Docker-compose (stable)

The project may work with other versions of these tools but they are not tested.

### Installation

Clone the repository, its submodule, and run the container
```bash
git clone git@github.com:gnosis/dex-services.git
cd dex-services
git submodule update --init
docker-compose up -d ganache-cli
(cd dex-contracts && yarn && npx truffle compile && npm run networks-inject && npx truffle migrate)
```

## StableX

The StableX system only consists of a simple service that queries the relevant auction information (orders and balances) directly from the blockchain. It then tries to find and submit a valid solution as soon as the order collection phase for a given auction ends.

The repo ships with a very naive solver, that can at the moment only match two orders between the fee token (*token0*) and another token if those orders overlap. A more sophisticated solver using a linear programming approach is not open sourced at the moment.

### Running StableX

```bash
docker-compose up stablex
```

In order to setup the environment and make a first deposit/order you can run:

```
cd dex-contracts
npx truffle exec scripts/stablex/setup_environment.js
npx truffle exec scripts/stablex/deposit.js --accountId=0 --tokenId=0 --amount=3000
npx truffle exec scripts/stablex/deposit.js --accountId=1 --tokenId=1 --amount=3000
npx truffle exec scripts/stablex/place_order.js --accountId=0 --buyToken=1 --sellToken=0 --minBuy=999 --maxSell=2000 --validFor=20
npx truffle exec scripts/stablex/place_order.js --accountId=1 --buyToken=0 --sellToken=1 --minBuy=1996 --maxSell=999 --validFor=20
```

It will then take up to 5 minutes (auctions close every 00, 05, 10 ... of the hour). On ganache you can expedite this process by running:

```
npx truffle exec scripts/stablex/close_auction.js
```

You should then see the docker container computing and applying a solution to the most recent auction. In order to withdraw your proceeds you can request a withdraw, wait for one auction for it to become claimable and claim it:

```
npx truffle exec scripts/stablex/request_withdraw.js --accountId=0 --tokenId=1 --amount=999
npx truffle exec scripts/stablex/close_auction.js
npx truffle exec scripts/stablex/claim_withdraw.js --accountId=0 --tokenId=1 
```

**Note:** Whenever stopping the `ganache-cli` service (e.g. by running `docker-compose down` you have to re-migrate the dex-contract before restarting `stablex`)

## dƒusion Snapp

The Snapp system consists of two main components: The *Graph Listener* which indexes and processes calldata that is emitted via EVM events into the off-chain data store, and the *Driver* who interacts with the smart contract based on the data it finds in storage (e.g. applying pending deposits/withdraws or settling an auction).

The *Graph Listener* registers for certain EVM events via a slightly trimmed down version of [The Graph](https://thegraph.com/).
The [dex smart contract](https://github.com/gnosis/dex-contracts) emits these events on user interaction (deposit, withdraw, order) as well as when the saved state root hash is updated (state transitions).

Upon receiving a relevant event from the contract, the event listener computes the implied changes to the underlying state. 
E.g. if a *deposit* event is received, the list of pending deposits is updated.
Similarly, if a *deposit state transition* event is received it updates the account balances based on the pending deposits that were included in the state transition.

The *Driver* watches state updates to the database and reads relevant data from the smart contract to decide when a state transition can be applied.
There are four types of state transitions:

- apply deposit
- apply withdraws
- find solution for optimization problem
- apply trade execution (according to the winning solution)

The *Driver* computes the updated root state according to the data it reads from the database and submits a state transition to the smart contract.

The *Driver* does not write into the database.
Instead, the smart contract emits an event, which the *Event Listener* receives. The *Event Listener* then applies the state transition based on the data emitted in the event and the existing state in the database.
It also updates the state in the database.

Note that the *Event Listener* is the only component writing into the database.
There are two main reasons for that:
1. **Scalability:** By using the *Single Writer Principle* we can scale access to the database layer much better and thus provide a data availability provider that can also be used by external participants of the system.
2. **Driver Competition:** We assume, there will be multiple systems (or at least multiple instances of this system) competing in optimization and driving the state machine forward. 
Thus, our data layer has to rely only on the data emitted by the EVM. It cannot assume that the *Driver* is aware of updating all available data stores.

More components, e.g. a watchtower to challenge invalid state transitions, will be added in the future.

### Running the driver/listener

```
docker-compose down && docker-compose up driver graph-listener truffle
```

This will start:
- ganache-cli, the local ethereum chain
- a truffle image compiling and deploying your smart contracts to the local chain
- postgres, the database storing the data of the snapp
- graph-listener, a listener pulling data from the ganache-cli and inserting it into postgres
- driver, a service calculating the new states and push these into the smart contract

You can see the current state of the theGraph DB by opening [localhost:8000](http://localhost:8000) and connecting to the default database (top right).
On the left side bar, under *Collections* select the collection you want to inspect, e.g. *accounts*.

In order to setup some testing accounts and make the first deposits (from account 3, of the third registered token with an amount of 18), run in the same repo the following scripts:

```bash
cd dex-contracts
npx truffle exec scripts/snapp/setup_environment.js
npx truffle exec scripts/snapp/deposit.js --accountId=1 --tokenId=1 --amount=18
npx truffle exec scripts/wait_seconds.js 181
```

To claim back the deposit, submit a withdraw request:

```bash
npx truffle exec scripts/snapp/request_withdraw.js --accountId=1 --tokenId=1 --amount=18
```

After 20 blocks have passed, the driver will apply the state transition and you should be able to claim back your funds:

```bash
npx truffle exec scripts/wait_seconds.js 181
npx truffle exec scripts/snapp/claim_withdraw.js --slot=0 --accountId=1 --tokenId=1
```

## Tests

You need the following dependencies installed locally in order to run the e2e tests:
- [jq](https://stedolan.github.io/jq/)

For end-to-end tests, run from the project root:

For StableX:

```bash
docker-compose down && docker-compose up stablex
e2e-tests-stablex-ganache.sh

export PK=... # Some private key with Rinkeby OWL, DAI and ETH (for gas)
docker-compose down && docker-compose -f docker-compose.yml -f docker-compose.rinkeby.yml up stablex
e2e-tests-stablex-rinkeby.sh
```

For dƒusion Snapp:

```bash
docker-compose down && docker-compose up
./test/e2e-tests-deposit-withdraw.sh

docker-compose down && docker-compose up
./test/e2e-tests-auction.sh

docker-compose down && docker-compose up
./test/e2e-tests-standing-order.sh
```

If end-to-end tests are failing, check the `docker-compose logs` and consider inspecting the DB state using the web interface.

To run unit tests:

```bash
cargo test
```

We also require `cargo clippy` and `cargo fmt` to pass for any PR to be merged.

## Running with linear optimization solver

For this to work, you will need to have read access to our AWS docker registry and have [awscli](https://aws.amazon.com/cli/) installed. Use this command to login:

```sh
$(aws ecr get-login --no-include-email)
```

Then specify the solver image you want to use as a build argument, e.g.: 

```sh
docker-compose build --build-arg SOLVER_BASE=163030813197.dkr.ecr.us-east-1.amazonaws.com/dex-solver:latest stablex
```

and add the following line to you `common.env` file:

```
LINEAR_OPTIMIZATION_SOLVER=1
```

Afterwards, when you run your environment e.g. with `docker-compose up stablex` the linear optimizer should be automatically used. Note that the e2e tests might no longer work, as their resolution depends on the naive and not the optimal solving strategy.

## Troubleshooting

### Logging

The driver uses `slog-envlogger` as a `slog` drain which means that logging filters can be controlled by the environment. To modify the logging filter, use the `DFUSION_LOG` environment variable:

```bash
# only log warnings except for 'driver::transport' module
DFUSION_LOG=warn,driver::transport=debug cargo run --bin stablex
```

More information on the logging filter syntax can be found in the `slog-envlogger` [documentation](https://docs.rs/slog-envlogger/2.2.0/slog_envlogger/).

### docker-compose build

If you have built the docker landscape before, and there are updates to the rust dependencies or other implementation details, you might have to rebuild your docker images (in particular if there is a new version of the linear optimization solver).

```bash
docker-compose build
```

### Different networks:

In order to start StableX for the Rinkeby network, make sure that the env variables in common-rinkeby.env are up to date and then start the specific docker:

```
docker-compose -f docker-compose.yml -f docker-compose.rinkeby.yml up stablex
```
