use ethcontract::web3::api::Web3;
use ethcontract::web3::futures::Future as F;
use ethcontract::web3::transports::Http;
use ethcontract::web3::types::U256;
use ethcontract::{Account, PrivateKey};

use futures::future::join_all;

use e2e::common::{wait_for_condition, FutureBuilderExt, FutureWaitExt};
use e2e::stablex::{close_auction, setup_stablex};
use e2e::{BatchExchange, IERC20};

use std::env;
use std::process::Command;
use std::time::Duration;

#[test]
fn test_with_ganache() {
    let (eloop, http) = Http::new("http://localhost:8545").expect("transport failed");
    eloop.into_remote();
    let web3 = Web3::new(http);
    let (instance, accounts, tokens) = setup_stablex(&web3, 3, 3, 100);

    // Dynamically fetching the id allows the test to be run multiple times,
    // even if other tokens have already been added
    let first_token_id = instance
        .token_address_to_id_map(tokens[0].address())
        .wait_and_expect("Cannot get first token id");

    let second_token_id = instance
        .token_address_to_id_map(tokens[1].address())
        .wait_and_expect("Cannot get second token id");
    instance
        .deposit(tokens[0].address(), 3_000_000.into())
        .from(Account::Local(accounts[0], None))
        .wait_and_expect("Failed to send first deposit");

    instance
        .deposit(tokens[1].address(), 3_000_000.into())
        .from(Account::Local(accounts[1], None))
        .wait_and_expect("Failed to send second deposit");

    let batch = instance
        .get_current_batch_id()
        .wait_and_expect("Cannot get batchId");

    instance
        .place_order(
            second_token_id,
            first_token_id,
            batch + 20,
            999_000.into(),
            2_000_000.into(),
        )
        .from(Account::Local(accounts[0], None))
        .wait_and_expect("Cannot place first order");

    instance
        .place_order(
            first_token_id,
            second_token_id,
            batch + 20,
            1_996_000.into(),
            999_000.into(),
        )
        .from(Account::Local(accounts[1], None))
        .wait_and_expect("Cannot place first order");
    close_auction(&web3, &instance);

    // wait for solver to submit solution
    wait_for_condition(|| {
        instance
            .get_current_objective_value()
            .wait_and_expect("Cannot get objective value")
            > U256::zero()
    })
    .expect("No non-trivial solution submitted");

    instance
        .request_withdraw(tokens[1].address(), 999_000.into())
        .from(Account::Local(accounts[0], None))
        .wait_and_expect("Cannot place request withdraw");
    close_auction(&web3, &instance);

    let balance_before = tokens[1]
        .balance_of(accounts[0])
        .wait_and_expect("Cannot get balance before");

    instance
        .withdraw(accounts[0], tokens[1].address())
        .wait_and_expect("Cannot withdraw");

    let balance_after = tokens[1]
        .balance_of(accounts[0])
        .wait_and_expect("Cannot get balance after");
    assert_eq!(balance_after - balance_before, 999_000.into())
}

#[test]
fn test_rinkeby() {
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
        .send_and_confirm(Duration::from_secs(1), 1);
    let second_approve = IERC20::at(&web3, token_b)
        .approve(instance.address(), 1_000_000.into())
        .nonce(nonce + 1)
        .gas(1_000_000.into())
        .gas_price(8_000_000_000u64.into())
        .from(account)
        .send_and_confirm(Duration::from_secs(1), 1);

    // Deposit Funds
    let first_deposit = instance
        .deposit(token_a, 1_000_000.into())
        .nonce(nonce + 2)
        .send_and_confirm(Duration::from_secs(1), 1);
    let second_deposit = instance
        .deposit(token_b, 1_000_000.into())
        .nonce(nonce + 3)
        .send_and_confirm(Duration::from_secs(1), 1);

    // Place orders
    let first_order = instance
        .place_order(0, 7, batch + 2, 1_000_000.into(), 10_000_000.into())
        .nonce(nonce + 4)
        .send_and_confirm(Duration::from_secs(1), 1);
    let second_order = instance
        .place_order(7, 0, batch + 1, 1_000_000.into(), 10_000_000.into())
        .nonce(nonce + 5)
        .send_and_confirm(Duration::from_secs(1), 1);

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

    // Wait for solution to be applied
    let sleep_time = instance
        .get_seconds_remaining_in_batch()
        .wait_and_expect("Cannot get seconds remaining in batch")
        .low_u64()
        + 30;

    println!("Sleeping {} seconds...", sleep_time);
    std::thread::sleep(Duration::from_secs(sleep_time));

    // Make sure there was no error
    let output = Command::new("docker-compose")
        .arg("logs")
        .output()
        .expect("failed to execute process");
    let logs = String::from_utf8(output.stdout).expect("failed to read logs");
    // Our logger prints log level with four characters, thus searching for ERRO
    assert!(!logs.to_lowercase().contains("erro"));
}
