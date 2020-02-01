# Guide to Running Tests

It is important to note that one must ensure that all dfusion related builds are recent (i.e. driver, graph-listener and their dependents). 

From within the project source run:

```sh
docker-compose down && docker-compose up driver graph-listener truffle
```

Once the driver has recognized the deployed contracts each of the test must be run individually as

```shell script
cargo test -p e2e snapp_<testname> -- --nocapture
```
The nocapture used here will display log statements while the test is running to help identify where the test is at.

where `<testname>` can be any of "deposit_withdraw", "auction" or "standing_order".

In order to run multiple tests some containers must be restarted and the database must be removed.
For example, the following sequence of commands (in separate/alternating terminals) will run all the tests. 
Notice that, although the travis file uses the bash script for restarting containers, this will not work locally.  

```shell script
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
