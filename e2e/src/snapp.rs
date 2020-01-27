use crate::*;

use crate::common::{
    approve, create_accounts_with_funded_tokens, wait_for_condition, FutureBuilderExt,
    FutureWaitExt, MAX_GAS,
};
use ethcontract::web3::api::Web3;
use ethcontract::web3::transports::Http;
use ethcontract::web3::types::H160;
use ethcontract::Account;

use dfusion_core::database::{DbInterface, GraphReader};

use graph::log::logger;
use graph_node_reader::Store as GraphNodeReader;

// Snapp contract artifacts
ethcontract::contract!("dex-contracts/build/contracts/SnappAuction.json");

pub fn setup_snapp(
    web3: &Web3<Http>,
    num_tokens: usize,
    num_users: usize,
) -> (SnappAuction, Vec<H160>, Vec<IERC20>, Box<dyn DbInterface>) {
    let graph_logger = logger(false);
    let postgres_url = "postgresql://dfusion:let-me-in@localhost/dfusion";
    let store_reader = GraphNodeReader::new(postgres_url.parse().unwrap(), &graph_logger);
    let db_instance = GraphReader::new(Box::new(store_reader));

    let (accounts, tokens) = create_accounts_with_funded_tokens(&web3, num_tokens, num_users);
    let mut instance =
        SnappAuction::deployed(&web3).wait_and_expect("Cannot get deployed SnappAuction");
    println!("Acquired contract instance {}", instance.address());
    instance.defaults_mut().gas = Some(MAX_GAS.into());
    approve(&tokens, instance.address(), &accounts);

    // Open Accounts
    for (i, account) in accounts.iter().enumerate() {
        instance
            .open_account(i as u64)
            .from(Account::Local(*account, None))
            .wait_and_expect("Cannot open account");
    }

    // Register Tokens
    for token in &tokens {
        instance
            .add_token(token.address())
            .wait_and_expect("Cannot register token");
    }
    (instance, accounts, tokens, Box::new(db_instance))
}

pub fn await_state_transition(instance: &SnappAuction, current_state: &[u8]) -> [u8; 32] {
    wait_for_condition(|| {
        instance
            .get_current_state_root()
            .wait_and_expect("Could not recover current state root")
            != current_state
    })
    .expect("No state change detected");

    instance
        .get_current_state_root()
        .wait_and_expect("Could not recover current state root")
}
