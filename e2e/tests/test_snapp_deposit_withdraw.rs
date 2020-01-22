use e2e::common::{wait_for, FutureWaitExt};
use e2e::snapp::{await_state_transition, setup_snapp};
use ethcontract::web3::api::Web3;
use ethcontract::web3::transports::Http;
use ethcontract::web3::types::{H256, U128};
use ethcontract::Account;
use std::str::FromStr;

#[test]
fn test_deposit_and_withdraw() {
    let (eloop, http) = Http::new("http://localhost:8545").expect("transport failed");
    eloop.into_remote();
    let web3 = Web3::new(http);
    let (instance, accounts, _tokens) = setup_snapp(&web3, 3, 3);

    println!("Acquired instance data");

    let previous_state_hash = instance
        .get_current_state_root()
        .call()
        .wait()
        .expect("Could not recover previous state hash");

    let deposit_amount = U128::from(18_000_000_000_000_000_000u128);
    instance
        .deposit(2, deposit_amount)
        .from(Account::Local(accounts[2], None))
        .send()
        .wait()
        .expect("Failed to send first deposit");

    // TODO - Query the graph for Event emission.

    wait_for(&web3, 181);

    // Check that contract was updated
    let expected_deposit_hash =
        H256::from_str("781cff80f5808a37f4c9009218c46af3d90920f82110129f6d925fafb3b23f2d").unwrap();

    let after_deposit_state = await_state_transition(&instance, &previous_state_hash);
    assert_eq!(
        expected_deposit_hash,
        H256::from_slice(&after_deposit_state)
    );

    // TODO - Check that the graph DB was updated
    // Query DB for expected hash - accountStates(where: {id: expected_state_hash}).balances[62] == deposit_amount
    // assert_eq!(graph_account_balance, deposit_amount);

    instance
        .request_withdrawal(2, deposit_amount)
        .from(Account::Local(accounts[2], None))
        .send()
        .wait()
        .expect("Failed to send first deposit");

    // Query DB to see that Withdraw Request was recorded
    // withdraws where accountId = 0x0....2
    wait_for(&web3, 181);
    let _expected_withdraw_hash =
        H256::from_str("7b738197bfe79b6d394499b0cac0186cdc2f65ae2239f2e9e3c698709c80cb67").unwrap();

    // Wait for state transition and get new state
    let _after_withdraw_state = await_state_transition(&instance, &after_deposit_state);

    // Check that DB updated.

    // Claim Withdraw
    // TODO - Construct merkle proof from state of accounts.
}
