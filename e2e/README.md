This crate contains tests and e2e scripts for gathering historic price estimate information.

## Guide to Running Tests

To run the stableX related tests locally,

### Ganache:

```sh
# T1:
ci/setup_contracts.sh

# T2:
cargo run -p driver -- --node-url http://localhost:8545 --network-id 5777 --private-key 4f3edf983ac636a65a842ce7c78d9aa706d3b113bce9c46f30d7d21715b23b1d --solver-type naive-solver --scheduler evm
# Wait for driver to start up

# T3:
cargo test -p e2e ganache -- --nocapture
# The test is over when this command exits.
```

### Rinkeby:

```sh
# T1:
ci/setup_contracts.sh

# T2:
# <private-key> is some private key with Rinkeby OWL, DAI and ETH (for gas)
cargo run -p driver -- --node-url https://node.rinkeby.gnosisdev.com/ --network-id 4 --private-key <private-key> --solver-type naive-solver --scheduler system
# Wait for driver to start up

# T3:
cargo test -p e2e rinkeby -- --nocapture
# The test is over when this command exits.
```

## Guide to Running E2E Scripts

There are two e2e scripts available for gathering `pricegraph` performance metrics by analyzing historing batches and submitted solutions. All scripts are run from the root of the repository.

### Historic Prices

This script estimates historic OWL prices for tokens and compares them to the results produced by the solver.

```
$ cargo run --release -p e2e --bin historic_prices -- --orderbook-file path/to/orderbook/file
```

### Historic Trades

This script analyses the historic trades on the exchange and compares the exchange rates to the `pricegraph` estimates.

```
$ cargo run --release -p e2e --bin historic_trades -- --orderbook-file path/to/orderbook/file
```
