Intro
=====
This repository contains the backend logic for the dfusion exchange based on the specification, see [here](github.com/gnosis/dex-research)

Architecture
============
<p align="center">
<img src="documentation/architecture.png" alt="dex-services architecture" width="500">
 </p>

The *Event Listener* registers for certain EVM events via the [Gnosis Trading DB](https://github.com/gnosis/pm-trading-db).
The dex smart contract emits these events on user interaction (deposit, withdraw, order) as well as when the saved state root hash is updated (state transitions).

Upon receiving a relevant event from the contract, the event listener computes the implied changes to the underlying state. 
E.g. if a *deposit* event is received, the list of pending deposits is updated.
Similarily, if a *deposit state transition* event is received it updates the account balances based on the pending deposits that were included in the state transition.

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

Install Instructions
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
ganache-cli, the local ethereum chain
mongodb, the data base storing the data of the snapp
listener, a listener pulling data from the ganache-cli and inserting it into mongodb
driver, a service calculating the new states and push these into the smart contract

You can see the current state of the mongodb by opening [localhost:3000](http://localhost:3000) and connecting to the default database (top right).
On the left side bar, under *Collections* select the collection you want to inspect, e.g. *accounts*.

In order to setup some testing accounts and make the first deposits (from account 3, of the third registered token with an amount of 18), run in the same repo the following scripts:

```bash
cd dex-contracts
truffle exec scripts/setup_environment.js
truffle exec scripts/deposit.js 3 3 18
truffle exec scripts/mineBlocks.js 21
```

To claim back the deposit, submit a withdraw request:

```bash
truffle exec scripts/withdraw.js 3 3 18
```

After 20 blocks have passed, the driver will apply the state transition and you should be able to claim back your funds:

```bash
truffle exec scripts/mineBlocks.js 21
truffle exec scripts/claim_withdraw.js 1 3 3
```

Tests
========

For end-to-end tests, run from the project root:

```bash
docker-compose down && docker-compose up
test/e2e-tests-deposit.sh
test/e2e-tests-withdraw.sh
```

If end-to-end tests are failing, check the `docker-compose logs` and consider inspecting the DB state using the web interface.

To run unit tests and lint for the *EventListener*:
```bash
mypy event_listener/dfusion_db/ --ignore-missing-imports --strict
pytest
```

To run unit tests for the *Driver*:
```bash
cd driver
cargo test --lib
```

Troubleshooting
============

#### docker-compose build
If you have built the docker landscape before, and there are updates to the smart contracts submodule (*dex-contracts/*), you have to rebuild your docker environment, for them to be picked up:

```bash
docker-compose build truffle
```

or rebuild everything if you are desparate (will take longer, but might solve other problems as well)

```bash
docker-compose build
```
