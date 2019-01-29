# Running pepper with snarks from this repo

## Will be updated soon, currently only docker config is supported

## Prerequisits:

(These prerequisits will soon be dockerized)

Install mongodb("0.3.2") and run it using:

```sh
sudo systemctl start mongodb

``` 
- do not use any authentification for the database.


Install rust & cargo ("1.31") - older versions would not be compatible with newest mongodb drivers.


## Setup:


Deploy a [SnappBase Contract](https://github.com/gnosis/dex-contracts):

Run a local test network
```bash
ganache-cli -b 1 -d
```

Go into the dex-contracts repo and run
```bash
truffle migrate 
```

## Running the code (itself):

In order to load the first data into the data base, we have a test prepared. Just run:


```sh
cargo test 

```

Running 

```sh
cargo run 

```
will start a listener for the data base and tires to push new states after applying deposits (Here it fails as the depositHash in the smart contract is just bytes32(0)".

## Running the code (with other dockers):

Start the listener:

```sh
cd dex-services

docker-compose up
```

Running 

```sh
cargo run 

```
will start a listener for the data base and to push new states after applying deposits.
