use crate::common::{
    approve, create_accounts_with_funded_tokens, wait_for, FutureBuilderExt, FutureWaitExt, MAX_GAS,
};
use contracts::{BatchExchange, TokenOWL, IERC20};
use ethcontract::{Account, Address, U256};
use services_core::contracts::Web3;

pub fn setup_stablex(
    web3: &Web3,
    num_tokens: usize,
    num_users: usize,
    token_minted: u32,
) -> (BatchExchange, Vec<Address>, Vec<IERC20>) {
    // Get all tokens but OWL in a generic way
    let (accounts, mut tokens) =
        create_accounts_with_funded_tokens(&web3, num_tokens - 1, num_users, token_minted);
    let mut instance =
        BatchExchange::deployed(&web3).wait_and_expect("Cannot get deployed BatchExchange");
    instance.defaults_mut().gas = Some(MAX_GAS.into());
    approve(&tokens, instance.address(), &accounts, token_minted);

    // Set up OWL manually
    let owl_address = instance
        .token_id_to_address_map(0)
        .wait_and_expect("Cannot get address of OWL Token");
    let owl = TokenOWL::at(web3, owl_address);
    owl.set_minter(accounts[0])
        .wait_and_expect("Cannot set minter");
    for account in &accounts {
        owl.mint_owl(*account, U256::exp10(22) * token_minted)
            .wait_and_expect("Cannot mint OWl");
        owl.approve(instance.address(), U256::exp10(22) * token_minted)
            .from(Account::Local(*account, None))
            .wait_and_expect("Cannot approve OWL for burning");
    }

    // token[0] is already added in constructor
    for token in &tokens {
        instance
            .add_token(token.address())
            .wait_and_expect("Cannot add token");
    }
    tokens.insert(0, IERC20::at(&web3, owl_address));
    (instance, accounts, tokens)
}

pub fn close_auction(web3: &Web3, instance: &BatchExchange) {
    let seconds_remaining = instance
        .get_seconds_remaining_in_batch()
        .wait_and_expect("Cannot get seconds remaining in batch");
    wait_for(web3, seconds_remaining.as_u32());
}
