use contracts::{ERC20Mintable, IERC20};
use ethcontract::{
    contract::{Deploy, DeployBuilder, Detokenizable, MethodBuilder, ViewMethodBuilder},
    errors::{DeployError, MethodError},
    transaction::TransactionResult,
    web3::Transport,
    Account, Address, U256,
};
use futures::future::{FutureExt as _, LocalBoxFuture};
use services_core::contracts::Web3;
use std::{
    fmt::Debug,
    future::Future,
    io::{Error, ErrorKind},
    time::{Duration, Instant},
};

pub const MAX_GAS: u32 = 2_000_000;

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
    T: Transport + 'static,
    R: Detokenizable + 'static,
{
    type Future = LocalBoxFuture<'static, Result<TransactionResult, MethodError>>;

    fn into_future(self) -> Self::Future {
        self.send().boxed_local()
    }
}

impl<T, R> FutureBuilderExt for ViewMethodBuilder<T, R>
where
    T: Transport + 'static,
    R: Detokenizable + 'static,
{
    type Future = LocalBoxFuture<'static, Result<R::Output, MethodError>>;

    fn into_future(self) -> Self::Future {
        self.call().boxed_local()
    }
}

impl<T, I> FutureBuilderExt for DeployBuilder<T, I>
where
    T: Transport + 'static,
    I: Deploy<T> + 'static,
{
    type Future = LocalBoxFuture<'static, Result<I, DeployError>>;

    fn into_future(self) -> Self::Future {
        self.deploy().boxed_local()
    }
}

pub fn wait_for(web3: &Web3, seconds: u32) {
    web3.transport()
        .execute("evm_increaseTime", vec![seconds.into()])
        .wait()
        .expect("Cannot increase time");
    web3.transport()
        .execute("evm_mine", vec![])
        .wait()
        .expect("Cannot mine to increase time");
}

pub fn wait_for_condition<C>(mut condition: C, deadline: Instant) -> Result<(), Error>
where
    C: FnMut() -> bool,
{
    while Instant::now() < deadline {
        if condition() {
            return Ok(());
        }
        std::thread::sleep(Duration::from_secs(1));
    }
    Err(Error::new(
        ErrorKind::TimedOut,
        "Condition not met before time limit",
    ))
}

pub fn create_accounts_with_funded_tokens(
    web3: &Web3,
    num_tokens: usize,
    num_users: usize,
    token_minted: u32,
) -> (Vec<Address>, Vec<IERC20>) {
    let accounts: Vec<Address> =
        web3.eth().accounts().wait().expect("get accounts failed")[..num_users].to_vec();

    let tokens: Vec<IERC20> = (0..num_tokens)
        .map(|_| {
            let token = ERC20Mintable::builder(web3)
                .confirmations(0)
                .wait_and_expect("Cannot deploy Mintable Token");
            for account in &accounts {
                token
                    .mint(*account, U256::exp10(22) * token_minted)
                    .wait_and_expect("Cannot mint token");
            }
            IERC20::at(&web3, token.address())
        })
        .collect();
    (accounts, tokens)
}

pub fn approve(tokens: &[IERC20], address: Address, accounts: &[Address], approval_amount: u32) {
    for account in accounts {
        for token in tokens {
            token
                .approve(address, U256::exp10(22) * approval_amount)
                .from(Account::Local(*account, None))
                .wait()
                .unwrap_or_else(|_| panic!("Cannot approve token {:x}", token.address()));
        }
    }
}
