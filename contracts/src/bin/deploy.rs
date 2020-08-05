//! Script to deploy Gnosis Protocol contracts to a local test network.
//! Additionally writes the deployed addresses to the `target` directory so that
//! they can be used by the build script.

use anyhow::{anyhow, bail, Context as _, Result};
use contracts::*;
use env_logger::Env;
use ethcontract::{Address, Http, Web3};
use futures::compat::Future01CompatExt as _;
use std::{
    fs, thread,
    time::{Duration, Instant},
};

fn main() {
    env_logger::init_from_env(Env::default().default_filter_or("warn,deploy=info"));

    if let Err(err) = futures::executor::block_on(run()) {
        log::error!("Error deploying contracts: {:?}", err);
        std::process::exit(-1);
    }
}

async fn run() -> Result<()> {
    const NODE_URL: &str = "http://localhost:8545";

    let (eloop, http) = Http::new(NODE_URL)?;
    let web3 = Web3::new(http);
    eloop.into_remote();

    log::info!("checking connection to local test node {}", NODE_URL);
    wait_for_node(&web3).await?;

    macro_rules! deploy {
            ($contract:ident) => { deploy!($contract ()) };
            ($contract:ident ( $($param:expr),* $(,)? )) => {{
                const NAME: &str = stringify!($contract);

                log::debug!("deploying {}...", NAME);
                let instance = $contract::builder(&web3 $(, $param)*)
                    .gas(8_000_000.into())
                    .deploy()
                    .await
                    .with_context(|| format!("failed to deploy {}", NAME))?;

                log::debug!(
                    "writing deployment to {}",
                    paths::contract_address_file(NAME).display(),
                );
                write_contract_address(stringify!($contract), instance.address())
                    .with_context(|| format!("failed to write contract address for {}", NAME))?;

                log::info!("deployed {} to {:?}", NAME, instance.address());
                instance
            }};
        }

    log::info!("deploying library contracts");
    let bi_map = deploy!(IdToAddressBiMap);
    let iterable_set = deploy!(IterableAppendOnlySet);

    log::info!("deploying fee token contracts");
    let owl = deploy!(TokenOWL);
    let owl_proxy = deploy!(TokenOWLProxy(owl.address()));

    log::info!("deploying exchange and viewer contracts");
    let exchange = deploy!(BatchExchange(
        batch_exchange::Libraries {
            id_to_address_bi_map: bi_map.address(),
            iterable_append_only_set: iterable_set.address(),
        },
        u16::max_value().into(),
        owl_proxy.address(),
    ));
    deploy!(BatchExchangeViewer(exchange.address()));

    Ok(())
}

/// Writes the deployed contract address to the workspace `target` directory.
fn write_contract_address(name: &str, address: Address) -> Result<()> {
    let path = paths::contract_address_file(name);
    let dir = path
        .parent()
        .ok_or_else(|| anyhow!("contract address path does not have a parent directory"))?;

    fs::create_dir_all(dir)?;
    fs::write(path, format!("{:?}", address))?;

    Ok(())
}

/// Waits for the local development node to become available. Returns an error
/// if the node does not become available after a certain amount of time.
async fn wait_for_node(web3: &Web3<Http>) -> Result<()> {
    const NODE_READY_TIMEOUT: Duration = Duration::from_secs(30);
    const NODE_READY_POLL_INTERVAL: Duration = Duration::from_secs(1);

    let start = Instant::now();
    while start.elapsed() < NODE_READY_TIMEOUT {
        if web3.eth().accounts().compat().await.is_ok() {
            return Ok(());
        }

        log::warn!(
            "node not responding, retrying in {}s",
            NODE_READY_POLL_INTERVAL.as_secs_f64(),
        );

        // NOTE: Usually a blocking call in a future is bad, but since we block
        // on this future right at the beginning and have no concurrent fibers,
        // it should be OK for this simple script.
        thread::sleep(NODE_READY_POLL_INTERVAL);
    }

    bail!(
        "Timed out waiting for node after {}s",
        NODE_READY_TIMEOUT.as_secs(),
    )
}
