use ethcontract::web3::api::Web3;
use ethcontract::web3::futures::Future as F;
use ethcontract::web3::transports::Http;
use ethcontract::web3::types::{H160, U256};
use ethcontract::web3::Transport;
use ethcontract::Account;

use std::future::Future;
use std::io::{Error, ErrorKind};

ethcontract::contract!("dex-contracts/build/contracts/BatchExchange.json");
ethcontract::contract!("dex-contracts/build/contracts/IERC20.json");
ethcontract::contract!("dex-contracts/build/contracts/IdToAddressBiMap.json");
ethcontract::contract!("dex-contracts/build/contracts/IterableAppendOnlySet.json");
ethcontract::contract!("dex-contracts/build/contracts/TokenOWL.json");
ethcontract::contract!("dex-contracts/build/contracts/ERC20Mintable.json");

pub trait FutureWaitExt: Future {
    fn wait(self) -> Self::Output;
}

impl<F> FutureWaitExt for F
where
    F: Future,
{
    fn wait(self) -> Self::Output {
        futures::executor::block_on(self)
    }
}

const TOKEN_MINTED: u32 = 100;
const MAX_GAS: u32 = 6_000_000;

pub fn setup(
    web3: &Web3<Http>,
    num_tokens: usize,
    num_users: usize,
) -> (BatchExchange, Vec<H160>, Vec<IERC20>) {
    let accounts: Vec<H160> =
        web3.eth().accounts().wait().expect("get accounts failed")[..num_users].to_vec();

    let mut instance = BatchExchange::deployed(&web3)
        .wait()
        .expect("Cannot get deployed BatchExchange");
    instance.defaults_mut().gas = Some(MAX_GAS.into());

    let owl_address = instance
        .token_id_to_address_map(0)
        .call()
        .wait()
        .expect("Cannot get address of OWL Token");
    let owl = TokenOWL::at(web3, owl_address);
    owl.set_minter(accounts[0])
        .send()
        .wait()
        .expect("Cannot set minter");
    for account in &accounts {
        owl.mint_owl(*account, U256::exp10(18) * TOKEN_MINTED)
            .send()
            .wait()
            .expect("Cannot mint OWl");
    }

    let tokens: Vec<IERC20> = vec![IERC20::at(&web3, owl_address)]
        .into_iter()
        .chain((1..num_tokens).map(|_| {
            let token = ERC20Mintable::builder(web3)
                .gas(MAX_GAS.into())
                .confirmations(0)
                .deploy()
                .wait()
                .expect("Cannot deploy Mintable Token");
            for account in &accounts {
                token
                    .mint(*account, U256::exp10(18) * TOKEN_MINTED)
                    .send()
                    .wait()
                    .expect("Cannot mint token");
            }
            IERC20::at(&web3, token.address())
        }))
        .collect();

    for account in &accounts {
        for token in &tokens {
            token
                .approve(instance.address(), U256::exp10(18) * TOKEN_MINTED)
                .from(Account::Local(*account, None))
                .send()
                .wait()
                .expect("Cannot approve OWL for burning");
        }
    }

    // token[0] is already addfed in constructor
    for token in &tokens[1..] {
        instance
            .add_token(token.address())
            .gas(MAX_GAS.into())
            .send()
            .wait()
            .expect("Cannot add token");
    }
    (instance, accounts, tokens)
}

pub fn wait_for(web3: &Web3<Http>, seconds: u32) {
    web3.transport()
        .execute("evm_increaseTime", vec![seconds.into()]);
    web3.transport().execute("evm_mine", vec![]);
}

pub fn close_auction(web3: &Web3<Http>, instance: &BatchExchange) {
    let seconds_remaining = instance
        .get_seconds_remaining_in_batch()
        .call()
        .wait()
        .expect("Cannot get seconds remaining in batch");
    wait_for(web3, seconds_remaining.low_u32());
}

pub fn wait_for_condition<C>(condition: C) -> Result<(), Error>
where
    C: Fn() -> bool,
{
    // Repeatedly check condition with 100ms sleep time in between tries (max ~30s)
    for _ in 0..300 {
        if condition() {
            return Ok(());
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    Err(Error::new(
        ErrorKind::TimedOut,
        "Condition not met before time limit",
    ))
}
