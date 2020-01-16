use crate::*;

use crate::common::{
    approve, create_accounts_with_funded_tokens, wait_for_condition, FutureWaitExt, MAX_GAS,
};
use ethcontract::web3::api::Web3;
use ethcontract::web3::transports::Http;
use ethcontract::web3::types::H160;
use ethcontract::Account;

pub fn setup_snapp(
    web3: &Web3<Http>,
    num_tokens: usize,
    num_users: usize,
) -> (SnappAuction, Vec<H160>, Vec<IERC20>) {
    let (accounts, tokens) = create_accounts_with_funded_tokens(&web3, num_tokens, num_users);
    let mut instance = SnappAuction::deployed(&web3)
        .wait()
        .expect("Cannot get deployed SnappAuction");
    instance.defaults_mut().gas = Some(MAX_GAS.into());
    approve(&tokens, instance.address(), &accounts);

    // Open Accounts
    for (i, account) in accounts.iter().enumerate() {
        instance
            .open_account(1 + i as u64)
            .from(Account::Local(*account, None))
            .send()
            .wait()
            .expect("Cannot open account");
    }

    // Register Tokens
    for token in &tokens {
        instance
            .add_token(token.address())
            .send()
            .wait()
            .expect("Cannot register token");
    }
    (instance, accounts, tokens)
}

pub fn await_state_transition(instance: &SnappAuction, current_state: &[u8]) -> [u8; 32] {
    wait_for_condition(|| {
        instance
            .get_current_state_root()
            .call()
            .wait()
            .expect("Could not recover current state root")
            != current_state
    })
    .expect("No state change detected");

    instance
        .get_current_state_root()
        .call()
        .wait()
        .expect("Could not recover current state root")
}
