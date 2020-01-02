use ethcontract::web3::api::Web3;
use ethcontract::web3::transports::Http;
use ethcontract::web3::types::U256;
use ethcontract::Account;

use common::FutureWaitExt;

mod common;

#[test]
fn test_stablex() {
    let (eloop, http) = Http::new("http://localhost:8545").expect("transport failed");
    eloop.into_remote();
    let web3 = Web3::new(http);
    let (instance, accounts, tokens) = common::setup(&web3, 3, 3);

    instance
        .deposit(tokens[0].address(), 3_000_000.into())
        .from(Account::Local(accounts[0], None))
        .send()
        .wait()
        .expect("Failed to send first deposit");

    instance
        .deposit(tokens[1].address(), 3_000_000.into())
        .from(Account::Local(accounts[1], None))
        .send()
        .wait()
        .expect("Failed to send second deposit");

    let batch = instance
        .get_current_batch_id()
        .call()
        .wait()
        .expect("Cannot get batchId");

    instance
        .place_order(1, 0, batch + 20, 999_000.into(), 2_000_000.into())
        .from(Account::Local(accounts[0], None))
        .send()
        .wait()
        .expect("Cannot place first order");

    instance
        .place_order(0, 1, batch + 20, 1_996_000.into(), 999_000.into())
        .from(Account::Local(accounts[1], None))
        .send()
        .wait()
        .expect("Cannot place first order");
    common::close_auction(&web3, &instance);

    // wait for solver to submit solution
    common::wait_for_condition(|| {
        instance
            .get_current_objective_value()
            .call()
            .wait()
            .expect("Cannot get objective value")
            > U256::zero()
    })
    .expect("No non-trivial solution submitted");

    instance
        .request_withdraw(tokens[1].address(), 999_000.into())
        .from(Account::Local(accounts[0], None))
        .send()
        .wait()
        .expect("Cannot place request withdraw");
    common::close_auction(&web3, &instance);

    let balance_before = tokens[1]
        .balance_of(accounts[0])
        .call()
        .wait()
        .expect("Cannot get balance before");

    instance
        .withdraw(accounts[0], tokens[1].address())
        .send()
        .wait()
        .expect("Cannot withdraw");

    let balance_after = tokens[1]
        .balance_of(accounts[0])
        .call()
        .wait()
        .expect("Cannot get balance after");
    assert_eq!(balance_after - balance_before, 999_000.into())
}
