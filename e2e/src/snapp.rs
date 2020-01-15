use crate::*;

use ethcontract::web3::api::Web3;
use ethcontract::web3::transports::Http;
use ethcontract::web3::types::{H160, U256};
use ethcontract::Account;

use crate::common::{
    approve, create_accounts_with_funded_tokens, wait_for, FutureWaitExt, MAX_GAS, TOKEN_MINTED,
};

pub fn setup_stablex(
    web3: &Web3<Http>,
    num_tokens: usize,
    num_users: usize,
) -> (SnappAuction, Vec<H160>, Vec<IERC20>) {

    let (accounts, mut tokens) =
        create_accounts_with_funded_tokens(&web3, num_tokens, num_users);
    let mut instance = SnappAuction::deployed(&web3)
        .wait()
        .expect("Cannot get deployed SnappAuction");
    instance.defaults_mut().gas = Some(MAX_GAS.into());
    approve(&tokens, instance.address(), &accounts);

    // Open Accounts
    for (i, account) in &accounts.iter().enumerate() {
        instance.open_account(i)
            .from(Account::Local(*account, None))
            .send()
            .wait()
            .expect(format!("Cannot open account {0} at index {1}", account, i));
    }

    // Register Tokens
    for token in &tokens {
        instance.add_token(token.address())
            .send()
            .wait()
            .expect("Cannot register token");
    }

    (instance, accounts, tokens)
}