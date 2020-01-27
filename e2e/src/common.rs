use crate::*;

use ethcontract::contract::{
    CallFuture, Deploy, DeployBuilder, DeployFuture, MethodBuilder, ViewMethodBuilder,
};
use ethcontract::transaction::SendFuture;
use ethcontract::web3::api::Web3;
use ethcontract::web3::contract::tokens::Detokenize;
use ethcontract::web3::futures::Future as F;
use ethcontract::web3::transports::Http;
use ethcontract::web3::types::{H160, U256};
use ethcontract::web3::Transport;
use ethcontract::Account;

use std::fmt::Debug;
use std::future::Future;
use std::io::{Error, ErrorKind};

pub const TOKEN_MINTED: u32 = 300;
pub const MAX_GAS: u32 = 6_000_000;

pub trait FutureWaitExt: Future + Sized {
    fn wait(self) -> Self::Output {
        futures::executor::block_on(self)
    }

    fn wait_and_expect<T, E>(self, message: &str) -> T
    where
        E: Debug,
        Self: Future<Output = Result<T, E>>,
    {
        self.wait().expect(message)
    }
}

impl<F> FutureWaitExt for F where F: Future {}

pub trait FutureBuilderExt: Sized {
    type Future: Future;

    fn into_future(self) -> Self::Future;

    fn wait(self) -> <Self::Future as Future>::Output {
        self.into_future().wait()
    }

    fn wait_and_expect<T, E>(self, message: &str) -> T
    where
        E: Debug,
        Self::Future: Future<Output = Result<T, E>>,
    {
        self.wait().expect(message)
    }
}

impl<T, R> FutureBuilderExt for MethodBuilder<T, R>
where
    T: Transport,
{
    type Future = SendFuture<T>;

    fn into_future(self) -> Self::Future {
        self.send()
    }
}

impl<T, R> FutureBuilderExt for ViewMethodBuilder<T, R>
where
    T: Transport,
    R: Detokenize,
{
    type Future = CallFuture<T, R>;

    fn into_future(self) -> Self::Future {
        self.call()
    }
}

impl<T, I> FutureBuilderExt for DeployBuilder<T, I>
where
    T: Transport,
    I: Deploy<T>,
{
    type Future = DeployFuture<T, I>;

    fn into_future(self) -> Self::Future {
        self.deploy()
    }
}

pub fn wait_for(web3: &Web3<Http>, seconds: u32) {
    web3.transport()
        .execute("evm_increaseTime", vec![seconds.into()]);
    web3.transport().execute("evm_mine", vec![]);
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

pub fn create_accounts_with_funded_tokens(
    web3: &Web3<Http>,
    num_tokens: usize,
    num_users: usize,
) -> (Vec<H160>, Vec<IERC20>) {
    let accounts: Vec<H160> =
        web3.eth().accounts().wait().expect("get accounts failed")[..num_users].to_vec();

    let tokens: Vec<IERC20> = (0..num_tokens)
        .map(|_| {
            let token = ERC20Mintable::builder(web3)
                .gas(MAX_GAS.into())
                .confirmations(0)
                .wait_and_expect("Cannot deploy Mintable Token");
            for account in &accounts {
                token
                    .mint(*account, U256::exp10(18) * TOKEN_MINTED)
                    .wait_and_expect("Cannot mint token");
            }
            IERC20::at(&web3, token.address())
        })
        .collect();
    (accounts, tokens)
}

pub fn approve(tokens: &[IERC20], address: H160, accounts: &[H160]) {
    for account in accounts {
        for token in tokens {
            token
                .approve(address, U256::exp10(18) * TOKEN_MINTED)
                .from(Account::Local(*account, None))
                .wait()
                .unwrap_or_else(|_| panic!("Cannot approve token {:x}", token.address()));
        }
    }
}
