# Guide to Running Tests

## Snapp Tests

Before proceeding, ensure that all dfusion-related docker images (i.e. driver, graph-listener) are recent and project submodules are updated. For further information on this please refer to the (installation guide)[https://github.com/gnosis/dex-services#installation] in the project root.  
Observe that the last two lines of the installation guide involve a manual deployment of deterministic ganache environment followed by a manual migration of the contracts. Both of these are unnecessary in our scenario since these are handled by the `truffle` container in the following command. 

From within the project source run:

```sh
docker-compose down && docker-compose up driver graph-listener truffle
```

Once the driver has recognized the deployed contracts each of the test must be run individually as

```sh
cargo test -p e2e snapp_<testname> -- --nocapture
```
The nocapture used here will display log statements while the test is running to help identify where the test is at.

where `<testname>` can be any of "deposit_withdraw", "auction" or "standing_order".

In order to run multiple tests some containers must be restarted and the database must be removed.
For example, the following sequence of commands (in separate/alternating terminals) will run all the tests. 
Notice that, although the travis file uses the bash script for restarting containers, this will not work locally (except on Linux).  

```sh
# T1:
docker-compose down && docker-compose up driver graph-listener truffle
# T2:
cargo test -p e2e snapp_deposit_withdraw -- --nocapture

# T1:
docker-compose down && docker-compose up driver graph-listener truffle
# T2:
cargo test -p e2e snapp_auction -- --nocapture

# T1:
docker-compose down && docker-compose up driver graph-listener truffle
# T2:
cargo test -p e2e snapp_standing_order -- --nocapture
```


## StableX Tests

To run the stableX related tests locally,

### Ganache:
```sh
# T1:
docker-compose down && docker-compose up stablex truffle
# T2:
cargo test -p e2e ganache
```

### Rinkeby:

```sh
# T1:
export PK=... # Some private key with Rinkeby OWL, DAI and ETH (for gas)
docker-compose down && docker-compose -f docker-compose.yml -f docker-compose.rinkeby.yml up stablex
# T2:
cargo test -p e2e rinkeby -- --nocapture
```
