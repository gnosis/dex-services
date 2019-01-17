# Running pepper with snarks from this repo


## Prerequisits:

(These prerequisits will soon be dockerized)

Install mongodb("0.3.2") and run it using:

```sh
sudo systemctl start mongodb

``` 
- do not use any authentification for the database.


Install rust & cargo ("1.31") - older versions would not be compatible with newest mongodb drivers.



## Setup:

In order to load the first data into the data base, run:


```sh
cargo run --bin database_setup

```

## Running the code:

Running 

```sh
cargo run --bin driver

```

will start a listener for the data base and run some c++ executions based on the data.
