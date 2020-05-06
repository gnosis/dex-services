use ethcontract::web3::api::Web3;
use ethcontract::web3::futures::Future as F;
use ethcontract::web3::transports::Http;
use ethcontract::{Account, PrivateKey, U256};

use futures::future::join_all;

use e2e::common::{wait_for_condition, FutureBuilderExt, FutureWaitExt};
use e2e::docker_logs;
use e2e::stablex::{close_auction, setup_stablex};
use e2e::{BatchExchange, IERC20};

use std::env;
use std::time::{Duration, Instant};

#[test]
fn test_with_ganache() {}

#[test]
fn test_rinkeby() {
    for _ in 0..10 {
        test_rinkeby_();
        std::thread::sleep(std::time::Duration::from_secs(300));
    }
}

fn test_rinkeby_() {
    // Setup instance and default tx params
    let (eloop, http) = Http::new("https://node.rinkeby.gnosisdev.com/").expect("transport failed");
    eloop.into_remote();
    let web3 = Web3::new(http);
    let mut instance =
        BatchExchange::deployed(&web3).wait_and_expect("Cannot get deployed Batch Exchange");
    let secret = {
        let private_key = env::var("PK").expect("PK env var not set");
        PrivateKey::from_hex_str(&private_key).expect("Cannot derive key")
    };
    let account = Account::Offline(secret, None);
    instance.defaults_mut().from = Some(account.clone());
    instance.defaults_mut().gas = Some(1_000_000.into());
    instance.defaults_mut().gas_price = Some(8_000_000_000u64.into());

    let nonce = web3
        .eth()
        .transaction_count(account.address(), None)
        .wait()
        .expect("Cannot get nonce");
    println!("Using account {:x} with nonce {}", account.address(), nonce);

    // Gather token and batch info
    let token_a = instance
        .token_id_to_address_map(0)
        .wait_and_expect("Cannot get first Token address");
    let token_b = instance
        .token_id_to_address_map(7)
        .wait_and_expect("Cannot get second Token address");
    let batch = instance
        .get_current_batch_id()
        .wait_and_expect("Cannot get batchId");

    // Approve Funds
    let first_approve = IERC20::at(&web3, token_a)
        .approve(instance.address(), 1_000_000.into())
        .nonce(nonce)
        .gas(1_000_000.into())
        .gas_price(8_000_000_000u64.into())
        .from(account.clone())
        .send();
    let second_approve = IERC20::at(&web3, token_b)
        .approve(instance.address(), 1_000_000.into())
        .nonce(nonce + 1)
        .gas(1_000_000.into())
        .gas_price(8_000_000_000u64.into())
        .from(account)
        .send();

    // Deposit Funds
    let first_deposit = instance
        .deposit(token_a, 1_000_000.into())
        .nonce(nonce + 2)
        .send();
    let second_deposit = instance
        .deposit(token_b, 1_000_000.into())
        .nonce(nonce + 3)
        .send();

    // Place orders
    let first_order = instance
        .place_order(0, 7, batch + 2, 1_000_000, 10_000_000)
        .nonce(nonce + 4)
        .send();
    let second_order = instance
        .place_order(7, 0, batch + 1, 1_000_000, 10_000_000)
        .nonce(nonce + 5)
        .send();

    // Wait for all transactions to be confirmed
    println!("Waiting for transactions to be confirmed");
    let results = join_all(vec![
        first_approve,
        second_approve,
        first_deposit,
        second_deposit,
        first_order,
        second_order,
    ])
    .wait();
    for (index, result) in results.into_iter().enumerate() {
        result.unwrap_or_else(|_| panic!("Tx #{} failed", index));
    }
}
