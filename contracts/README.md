# Gnosis Protocol Contracts

This crate contains `ethcontract` generated bindings to the Gnosis Protocol
contracts. Additionally it includes a `deploy` script for deploying contracts
to a test-net for E2E testing.

## `deploy` Script

A `[[bin]]` script for deploying Gnosis Protocol contracts to a test network.
This script requires a test node such as Ganache listening on `127.0.0.1:8545`.
It can be run from the repository root:

```
$ (cd contracts; cargo run -p contracts --bin deploy --features bin)
   Compiling contracts v0.1.0 (/var/home/nlordell/Developer/dex-services/contracts)
    Finished dev [unoptimized + debuginfo] target(s) in 4.87s
     Running `/var/home/nlordell/Developer/dex-services/target/debug/deploy`
[2020-08-05T12:43:07Z INFO  deploy] checking connection to local test node http://localhost:8545
[2020-08-05T12:43:07Z INFO  deploy] deploying library contracts
[2020-08-05T12:43:07Z INFO  deploy] deployed IdToAddressBiMap to 0x65dbc7c034b644401ad2a1f1ed8b284ae41e56bc
[2020-08-05T12:43:07Z INFO  deploy] deployed IterableAppendOnlySet to 0xc27e5a197f7f1c794bcd348f673d80687eff83a6
[2020-08-05T12:43:07Z INFO  deploy] deploying fee token contracts
[2020-08-05T12:43:07Z INFO  deploy] deployed TokenOWL to 0x8f534a8084d9306527ff15c919f7fa691c0ca856
[2020-08-05T12:43:07Z INFO  deploy] deployed TokenOWLProxy to 0x445d64d63f42401c8c7ff846d2a92edd2b31417b
[2020-08-05T12:43:07Z INFO  deploy] deploying exchange and viewer contracts
[2020-08-05T12:43:07Z INFO  deploy] deployed BatchExchange to 0x4fab5591ff63d133854e937ad4568afef5264e6a
[2020-08-05T12:43:07Z INFO  deploy] deployed BatchExchangeViewer to 0x63f7170efe984d40d1a435a917b99c71ad645901
```

This will generate `$CONTRACT_NAME.addr` files in the `target/deploy` directory.
The `build.rs` script uses these files to inject test network deployed addresses
into the generated bindings so `Contract::deployed()` methods work as expected
for E2E tests when connected to a local network.

Note that the `contracts` crate needs to be re-built after running the `deploy`
script to generate bindings with the injected test network addresses. This is
done automatically on `cargo build` by leveraging the `cargo:rerun-if-changed`
build script feature.
