# Guide to Running Tests

To run the stableX related tests locally,

## Ganache:

```sh
# T1:
docker-compose down && docker-compose up stablex truffle
# T2:
cargo test -p e2e ganache -- --nocapture
```

## Rinkeby:

```sh
# T1:
export PRIVATE_KEY=... # Some private key with Rinkeby OWL, DAI and ETH (for gas)
docker-compose down && docker-compose -f docker-compose.yml -f docker-compose.rinkeby.yml up stablex
# T2:
cargo test -p e2e rinkeby -- --nocapture
```
