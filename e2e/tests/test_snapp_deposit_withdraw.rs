use ethcontract::web3::api::Web3;
use ethcontract::web3::futures::Future as F;
use ethcontract::web3::transports::Http;
use ethcontract::web3::types::U256;
use ethcontract::{ethsign, Account, SecretKey, H256};

use futures::future::join_all;

use e2e::common::{wait_for_condition, FutureWaitExt};
use e2e::snapp::{close_auction, setup_snapp};
use e2e::IERC20;

use std::env;
use std::process::Command;
use std::time::Duration;

#[test]
fn test_deposit_and_withdraw() {
    let (eloop, http) = Http::new("http://localhost:8545").expect("transport failed");
    eloop.into_remote();
    let web3 = Web3::new(http);
    let (instance, accounts, tokens) = setup_snapp(&web3, 3, 3);

    let deposit_amount = 18_000_000_000_000_000_000u128;
    instance
        .deposit(tokens[2].address(), deposit_amount)
        .from(Account::Local(accounts[2], None))
        .send()
        .wait()
        .expect("Failed to send first deposit");

    // Query the graph for Event emission.

    // Advance time to finalize batch
    // TODO - write close auction
    // npx truffle exec scripts/wait_seconds.js 181

    // Check that contract was updated
    let expected_state_hash = "77b01abfbad57cb7a1344b12709603ea3b9ad803ef5ea09814ca212748f54733";
    let actual_state_hash = instance.get_current_state_root().call().wait().expect("Could not recover current state root");
    // assert_eq(expected_state_hash, actual_state_hash);

    // Check that the graph DB was updated
    // Query DB for expected hash - accountStates(where: {id: expected_state_hash}).balances[62] == deposit_amount
    // assert_eq!(graph_account_balance, deposit_amount);

    instance
        .request_withdraw(tokens[2].address(), deposit_amount)
        .from(Account::Local(accounts[2], None))
        .send()
        .wait()
        .expect("Failed to send first deposit");


}