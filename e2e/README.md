# Guide to Running Tests

To run the stableX related tests locally,

## Ganache:

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

## Rinkeby:

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
