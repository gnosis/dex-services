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

In order to load the first data into the data base, we have a test prepared. Just run:


```sh
cargo test -- --nocapture

```

## Running the code:

Running 

```sh
cargo run 

```

will start a listener for the data base and run some c++ executions based on the data.
