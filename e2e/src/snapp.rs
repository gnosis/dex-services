use crate::*;

use crate::common::{
    approve, create_accounts_with_funded_tokens, wait_for_condition, FutureBuilderExt,
    FutureWaitExt, MAX_GAS,
};
use ethcontract::web3::api::Web3;
use ethcontract::web3::transports::Http;
use ethcontract::web3::types::H160;
use ethcontract::{Account, H256, U256};

use dfusion_core::database::{DbInterface, GraphReader};

use crate::auction_bid::AuctionBid;
use dfusion_core::models::AccountState;
use graph::log::logger;
use graph_node_reader::Store as GraphNodeReader;
use std::rc::Rc;
use std::str::FromStr;

// Snapp contract artifacts
ethcontract::contract!("dex-contracts/build/contracts/SnappAuction.json");

pub fn setup_snapp(
    web3: &Web3<Http>,
    num_tokens: usize,
    num_users: usize,
    deposit_amount: u32,
) -> (SnappAuction, Vec<H160>, Vec<IERC20>, Rc<dyn DbInterface>) {
    let graph_logger = logger(false);
    let postgres_url = "postgresql://dfusion:let-me-in@localhost/dfusion";
    let store_reader = GraphNodeReader::new(postgres_url.parse().unwrap(), &graph_logger);
    let db_instance = GraphReader::new(Box::new(store_reader));

    let (accounts, tokens) =
        create_accounts_with_funded_tokens(&web3, num_tokens, num_users, deposit_amount);
    let mut instance =
        SnappAuction::deployed(&web3).wait_and_expect("Cannot get deployed SnappAuction");
    println!("Acquired contract instance {}", instance.address());
    instance.defaults_mut().gas = Some(MAX_GAS.into());
    approve(&tokens, instance.address(), &accounts, deposit_amount);

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
    (instance, accounts, tokens, Rc::new(db_instance))
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

pub fn await_and_fetch_auction_bid(instance: &SnappAuction, auction_index: U256) -> AuctionBid {
    let mut res = AuctionBid(
        instance
            .auctions(auction_index)
            .wait_and_expect("No auction bid detected on smart contract"),
    );

    wait_for_condition(|| {
        let bid = instance
            .auctions(auction_index)
            .wait_and_expect("No auction bid detected on smart contract");
        if bid.3 != H160::from_str("0000000000000000000000000000000000000000").unwrap() {
            res = AuctionBid(bid);
            true
        } else {
            false
        }
    })
    .expect("Did not detect bid placement in auction");
    res
}

pub fn await_and_fetch_new_account_state(
    db: Rc<dyn DbInterface>,
    tentative_state: H256,
) -> AccountState {
    wait_for_condition(|| db.get_balances_for_state_root(&tentative_state).is_ok())
        .expect("Did not detect account update in DB");
    db.get_balances_for_state_root(&tentative_state).unwrap()
}
