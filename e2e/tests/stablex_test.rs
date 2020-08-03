use contracts::{BatchExchange, IERC20};
use e2e::{
    common::{wait_for_condition, FutureBuilderExt as _, FutureWaitExt as _},
    docker_logs,
    stablex::{close_auction, setup_stablex},
};
use ethcontract::{Account, PrivateKey, U256};
use futures::future::{join_all, FutureExt as _};
use services_core::{contracts::Web3, http::HttpFactory};
use std::{
    env,
    time::{Duration, Instant},
};

fn web3(url: &str) -> Web3 {
    services_core::contracts::web3_provider(&HttpFactory::default(), url, Duration::from_secs(10))
        .expect("transport failed")
}

#[test]
fn test_with_ganache() {
    let web3 = web3("http://localhost:8545");
    let (instance, accounts, tokens) = setup_stablex(&web3, 3, 3, 100);

    // Dynamically fetching the id allows the test to be run multiple times,
    // even if other tokens have already been added
    let first_token_id = instance
        .token_address_to_id_map(tokens[0].address())
        .wait_and_expect("Cannot get first token id");

    let second_token_id = instance
        .token_address_to_id_map(tokens[1].address())
        .wait_and_expect("Cannot get second token id");

    // Using realistic prices helps non naive solvers find a solution in case
    // they filter out orders that are extremely small.
    let usd_price_in_fee = 10u128.pow(18);

    instance
        .deposit(tokens[0].address(), (3000 * usd_price_in_fee).into())
        .from(Account::Local(accounts[0], None))
        .wait_and_expect("Failed to send first deposit");

    instance
        .deposit(tokens[1].address(), (3000 * usd_price_in_fee).into())
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
            999 * usd_price_in_fee,
            2_000 * usd_price_in_fee,
        )
        .from(Account::Local(accounts[0], None))
        .wait_and_expect("Cannot place first order");

    instance
        .place_order(
            first_token_id,
            second_token_id,
            batch + 20,
            1_996 * usd_price_in_fee,
            999 * usd_price_in_fee,
        )
        .from(Account::Local(accounts[1], None))
        .wait_and_expect("Cannot place first order");
    close_auction(&web3, &instance);

    // wait for solver to submit solution
    wait_for_condition(
        || {
            instance
                .get_current_objective_value()
                .wait_and_expect("Cannot get objective value")
                > U256::zero()
        },
        Instant::now() + Duration::from_secs(30),
    )
    .expect("No non-trivial solution submitted");

    instance
        .request_withdraw(tokens[1].address(), (999 * usd_price_in_fee).into())
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
    let balance_change = (balance_after - balance_before).as_u128();
    let expected_balance_change = 999 * usd_price_in_fee;
    // With a non naive solver it is possible that the trade does not exactly
    // match what is expected.
    let allowed_difference = usd_price_in_fee;
    let difference = ((balance_change as i128) - (expected_balance_change as i128)).abs();
    assert!(difference < allowed_difference as i128);
}

#[test]
fn test_rinkeby() {
    // Setup instance and default tx params
    let web3 = web3("https://node.rinkeby.gnosisdev.com/");
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
        .send()
        .boxed();
    let second_approve = IERC20::at(&web3, token_b)
        .approve(instance.address(), 1_000_000.into())
        .nonce(nonce + 1)
        .gas(1_000_000.into())
        .gas_price(8_000_000_000u64.into())
        .from(account)
        .send()
        .boxed();

    // Deposit Funds
    let first_deposit = instance
        .deposit(token_a, 1_000_000.into())
        .nonce(nonce + 2)
        .send()
        .boxed();
    let second_deposit = instance
        .deposit(token_b, 1_000_000.into())
        .nonce(nonce + 3)
        .send()
        .boxed();

    // Place orders
    let first_order = instance
        .place_order(0, 7, batch + 2, 1_000_000, 10_000_000)
        .nonce(nonce + 4)
        .send()
        .boxed();
    let second_order = instance
        .place_order(7, 0, batch + 1, 1_000_000, 10_000_000)
        .nonce(nonce + 5)
        .send()
        .boxed();

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
        + 60;

    println!("Sleeping {} seconds...", sleep_time);
    std::thread::sleep(Duration::from_secs(sleep_time));

    docker_logs::assert_no_errors_logged("dex-services_stablex_1");
}
