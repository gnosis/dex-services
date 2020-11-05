[![Build Status](https://travis-ci.com/gnosis/dex-services.svg?branch=master)](https://travis-ci.com/gnosis/dex-services)
[![Coverage Status](https://coveralls.io/repos/github/gnosis/dex-services/badge.svg?branch=master)](https://coveralls.io/github/gnosis/dex-services)


# Contents
1. [Introduction](#introduction)
2. [Getting Started](#getting-started)
    1. [Requirements](#requirements)
    2. [Installation](#setup)
3. [Batch Exchange](#batchexchange)
    1. [Running](#running-batchexchange)
4. [Testing](#tests)
    1. [End to End](#end-to-end-tests)
    2. [Unit tests](#unit-tests)
5. [Open Solver](#running-with-open-solver)
6. [Optimization Solver](#running-with-linear-optimization-solver)
7. [Configuration](#configuration)
    1. [Orderbook Filtering](#orderbook-filter-example)
8. [Troubleshooting](#troubleshooting)
    1. [Logging](#logging)
    2. [Docker Compose](#docker-compose-build)
    3. [Different Networks](#different-networks)

---

## Introduction

This repository contains the backend logic for the Gnosis Protocol based on [this specification](https://github.com/gnosis/dex-research).

## Getting Started

### Requirements

- Rust (stable)
- Docker and Docker-compose (stable)

The project may work with other versions of these tools, but they are not tested.

### Setup

Clone the repository. To run against a local test-chain use the following commands to deploy a version of the smart contracts:

```bash
git clone git@github.com:gnosis/dex-services.git
cd dex-services
docker-compose up -d ganache-cli
(cd contracts; cargo run --bin deploy --features bin)
```

Running against public chains (e.g. Rinkeby/Mainnet) does not require any extra setup.

## BatchExchange

The BatchExchange system only consists of a simple service that queries the relevant auction information (orders and balances) directly from the blockchain. It then tries to find and submit a valid solution as soon as the order collection phase for a given auction ends.

The repo ships with a very naive solver, that can at the moment only match two orders between the fee token (*token0*) and another token if those orders overlap. A slightly more sophisticated solver allowing to match multiple orders between two directly overlapping tokens (without the restriction that one has to be a fee token) can be found [here](https://github.com/gnosis/dex-open-solver). We also developed a solver that uses a mixed integer programming approach, however this one is not open sourced at the moment. In order to implement a custom solver, check the smart contract for the required constraints in the `submitSolution` method.

### Running BatchExchange

You can run the rust binary locally (without docker). For that you will have to export the following environment variables:
- NODE_URL (for test environments this is usually http://localhost:8545. You can use an [Infura](https://infura.io/) node for rinkeby/mainnet)
- NETWORK_ID (chainId, e.g. 5777 for ganache, 4 for rinkeby, 1 for mainnet)
- PRIVATE_KEY (the hex key without leading 0x that should be used to sign transactions. Needs to be funded with eth for gas)

```bash
cargo run --bin driver
```

## Tests

### End-to-End Tests

For end-to-end tests, please consult the guide in [e2e/README](e2e/README.md).

### Unit Tests

To run unit tests:

```bash
cargo test
```

We also require `cargo clippy` and `cargo fmt` to pass for any PR to be merged.

## Running with open solver

If you are running ubuntu you can checkout the [open solver](https://github.com/gnosis/dex-open-solver) in `/app/open_solver` on your host machine.
However, it is likely more convenient to use the provided docker setup.
From the root directory of the repository, run:

```
docker-compose build --build-arg SOLVER_BASE=gnosispm/dex-open-solver:master stablex-debug
docker-compose run [-v $(PWD)/:/app/dex-services] stablex-debug
```

From within the container run

```
cargo run --bin driver -- --solver-type OpenSolver
```

The `-v` argument is optional will mount the repository from your host filesystem inside the container, so that you can still perform code changes locally without having to rebuild the container.
This also allows you to use orderbook files that have been synced previously and have the container write the updated version back to a common directory.

## Running with linear optimization solver

For this to work, you will need to have read access to our AWS docker registry and have [awscli](https://aws.amazon.com/cli/) installed. Use this command to login:

```sh
$(aws ecr get-login --no-include-email)
```

Then specify the solver image you want to use as a build argument, e.g.:

```sh
docker-compose build --build-arg SOLVER_BASE=163030813197.dkr.ecr.eu-central-1.amazonaws.com/dex-solver:master stablex-debug
```

Afterwards, when you run your environment as above, the linear optimizer should be automatically used.
Note that the e2e tests might no longer work, as their resolution depends on the naive and not the optimal solving strategy.

## Configuration

The binary can be configured via command line options and environment variables: `cargo run -- --help`

```
driver 0.1.0
Gnosis Exchange protocol driver.

USAGE:
    driver [OPTIONS] --node-url <node-url> --private-key <private-key>

FLAGS:
    -h, --help
            Prints help information

    -V, --version
            Prints version information


OPTIONS:
        --auction-data-page-size <auction-data-page-size>
            Specify the number of blocks to fetch events for at a time for constructing the orderbook for the solver
            [env: AUCTION_DATA_PAGE_SIZE=]  [default: 500]
        --earliest-solution-submit-time <earliest-solution-submit-time>
            The earliest offset from the start of a batch in seconds at which point we should submit the solution. This
            is useful when there are multiple solvers one of provides solutions more often but also worse solutions than
            the others. By submitting its solutions later we avoid its solution getting reverted by a better one which
            saves gas [env: EARLIEST_SOLUTION_SUBMIT_TIME=]  [default: 0]
        --economic-viability-min-avg-fee-factor <economic-viability-min-avg-fee-factor>
            We multiply the economically viable min average fee by this amount to ensure that if a solution has this
            minimum amount it will still be end up economically viable even when the gas or native token price moves
            slightly between solution computation and submission [env: ECONOMIC_VIABILITY_MIN_AVG_FEE_FACTOR=]
            [default: 1.1]
        --economic-viability-strategy <economic-viability-strategy>
            How to calculate the economic viability constraints. `Static`: Use fallback_min_avg_fee_per_order and
            fallback_max_gas_price. `Dynamic`: Use current native token price, gas price and subsidy factor. `Combined`:
            Use the better (lower min-avg-fee) of the above [env: ECONOMIC_VIABILITY_STRATEGY=]  [default: Dynamic]
            [possible values: Static, Dynamic, Combined]
        --economic-viability-subsidy-factor <economic-viability-subsidy-factor>
            Subsidy factor used to compute the minimum average fee per order in a solution as well as the gas cap for
            economically viable solution [env: ECONOMIC_VIABILITY_SUBSIDY_FACTOR=]  [default: 0.0]
        --http-timeout <http-timeout>
            The default timeout in milliseconds of HTTP requests to remote services such as the Gnosis Safe gas station
            and exchange REST APIs for fetching price estimates [env: HTTP_TIMEOUT=]  [default: 10000]
        --latest-solution-submit-time <latest-solution-submit-time>
            The offset from the start of the batch to cap the solver's execution time [env:
            LATEST_SOLUTION_SUBMIT_TIME=]  [default: 210]
        --log-filter <log-filter>
            The log filter to use.
            
            This follows the `slog-envlogger` syntax (e.g. 'info,driver=debug'). [env: LOG_FILTER=]  [default:
            warn,driver=info,services_core=info]
        --native-token-id <native-token-id>
            ID for the token which is used to pay network transaction fees on the target chain (e.g. WETH on mainnet,
            DAI on xDAI) [env: NATIVE_TOKEN_ID=]  [default: 1]
    -n, --node-url <node-url>
            The Ethereum node URL to connect to. Make sure that the node allows for queries without a gas limit to be
            able to fetch the orderbook [env: NODE_URL=]
        --orderbook-file <orderbook-file>
            Use an orderbook file for persisting an event cache in order to speed up the startup time [env:
            ORDERBOOK_FILE=]
        --orderbook-filter <orderbook-filter>
            JSON encoded object of which tokens/orders to ignore.
            
            For example: '{ "tokens": {"Whitelist": [1, 2]}, "users": { "0x7b60655Ca240AC6c76dD29c13C45BEd969Ee6F0A": {
            "OrderIds": [0, 1] }, "0x7b60655Ca240AC6c76dD29c13C45BEd969Ee6F0B": "All" } }' More examples can be found in
            the tests of orderbook/filtered_orderboook.rs [env: ORDERBOOK_FILTER=]  [default: {}]
        --price-source-update-interval <price-source-update-interval>
            Time interval in seconds in which price sources should be updated [env: PRICE_SOURCE_UPDATE_INTERVAL=]
            [default: 300]
    -k, --private-key <private-key>
            The private key used by the driver to sign transactions [env: PRIVATE_KEY]

        --rpc-timeout <rpc-timeout>
            The timeout in milliseconds of web3 JSON RPC calls, defaults to 10000ms [env: RPC_TIMEOUT=]  [default:
            10000]
        --scheduler <scheduler>
            The kind of scheduler to use [env: SCHEDULER=]  [default: System]  [possible values: System, Evm]

        --solver-internal-optimizer <solver-internal-optimizer>
            Which internal optimizer the solver should use. It is passed as `--solver` to the solver. Choices are "scip"
            and "gurobi" [env: SOLVER_INTERNAL_OPTIMIZER=]  [default: Scip]  [possible values: Scip, Gurobi]
        --solver-type <solver-type>
            Which style of solver to use. Can be one of: 'NaiveSolver' for the naive solver; 'StandardSolver' for mixed
            integer programming solver; 'FallbackSolver' for a more conservative solver than the standard solver;
            'BestRingSolver' for a solver searching only for the best ring; 'OpenSolver' for the open-source solver
            [env: SOLVER_TYPE=]  [default: NaiveSolver]  [possible values: NaiveSolver, StandardSolver, OpenSolver,
            BestRingSolver]
        --static-max-gas-price <static-max-gas-price>
            The static max gas price fee per order used for the Static strategy [env: STATIC_MAX_GAS_PRICE=]

        --static-min-avg-fee-per-order <static-min-avg-fee-per-order>
            The static minimum average fee per order used for the Static strategy [env: STATIC_MIN_AVG_FEE_PER_ORDER=]

        --target-start-solve-time <target-start-solve-time>
            The offset from the start of a batch in seconds at which point we should start solving [env:
            TARGET_START_SOLVE_TIME=]  [default: 30]
        --token-data <token-data>
            JSON encoded backup token information to provide to the solver.
            
            For example: '{ "T0001": { "address": "0xc02aaa39b223fe8d0a0e5c4f27ead9083c756cc2", "alias": "WETH",
            "decimals": 18, "externalPrice": 200000000000000000000, }, "T0004": { "address":
            "0x0000000000000000000000000000000000000000", "alias": "USDC", "decimals": 6, "externalPrice":
            1000000000000000000000000000000, } }' [env: TOKEN_DATA=]  [default: {}]
        --use-external-price-source <use-external-price-source>
            Whether to rely on external price sources (e.g. 1Inch, Kraken etc) when estimating token prices [env:
            USE_EXTERNAL_PRICE_SOURCE=]  [default: true]
```

### Orderbook Filter Example

```json
{
  "tokens": [1, 2],
  "users": {
    "0x7b60655Ca240AC6c76dD29c13C45BEd969Ee6F0A": { "OrderIds": [0, 1] },
    "0x7b60655Ca240AC6c76dD29c13C45BEd969Ee6F0B": "All"
  }
}
```

blacklists all orders that contain token 1 & 2, all orders of _0x...B_ and orderId 0 & 1 or _0x...A_

### Command-Line Configuration

The driver also supports configuration by directly passing in command-line arguments. Run the following to get more information on all supported command-line options:

```
cargo run -- --help
```

The command-line help output also specifies which arguments map to which of the environment variables specified above.

## Troubleshooting

### Logging

The driver uses `slog-envlogger` as a `slog` drain which means that logging filters can be controlled by the environment. To modify the logging filter, use the `DFUSION_LOG` environment variable:

```bash
# only log warnings except for 'driver::transport' module
DFUSION_LOG=warn,driver::transport=debug cargo run
```

More information on the logging filter syntax can be found in the `slog-envlogger` [documentation](https://docs.rs/slog-envlogger/2.2.0/slog_envlogger/).

### docker-compose build

If you have built the docker landscape before, and there are updates to the rust dependencies or other implementation details, you might have to rebuild your docker images (in particular if there is a new version of the dependent optimization solver).

```bash
docker-compose build
```

### Different networks:

In order to start BatchExchange for the Rinkeby network, make sure that the env variables in common-rinkeby.env are up to date and then start the specific docker:

```
docker-compose -f docker-compose.yml -f driver/docker-compose.rinkeby.yml up stablex-debug
```

For mainnet,

```
docker-compose -f docker-compose.yml -f driver/docker-compose.mainnet.yml up stablex-debug
```
