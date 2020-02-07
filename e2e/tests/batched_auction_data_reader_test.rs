use driver::orderbook::{PaginatedStableXOrderBookReader, StableXOrderBookReading};
use e2e::common::FutureBuilderExt;
use e2e::stablex::setup_stablex;
use ethcontract::web3::api::Web3;
use ethcontract::web3::transports::Http;
use ethcontract::Account;

#[test]
fn test_with_ganache() {
    let (eloop, http) = Http::new("http://localhost:8545").expect("transport failed");
    eloop.into_remote();
    let web3 = Web3::new(http);
    let (instance, accounts, tokens) = setup_stablex(&web3, 3, 3, 100);

    // Dynamically fetching the id allows the test to be run multiple times,
    // even if other tokens have already been added
    let first_token_id = instance
        .token_address_to_id_map(tokens[0].address())
        .wait_and_expect("Cannot get first token id");

    let second_token_id = instance
        .token_address_to_id_map(tokens[1].address())
        .wait_and_expect("Cannot get second token id");
    instance
        .deposit(tokens[0].address(), 3_000_000.into())
        .from(Account::Local(accounts[0], None))
        .wait_and_expect("Failed to send first deposit");

    instance
        .deposit(tokens[1].address(), 3_000_000.into())
        .from(Account::Local(accounts[1], None))
        .wait_and_expect("Failed to send second deposit");

    let batch = instance
        .get_current_batch_id()
        .wait_and_expect("Cannot get batchId");

    for i in 0..100 {
        instance
            .place_order(
                first_token_id,
                second_token_id,
                batch + 20,
                i.into(),
                (i + 1).into(),
            )
            .from(Account::Local(accounts[0], None))
            .wait_and_expect("cannot place order");
    }

    let reader = PaginatedStableXOrderBookReader::new(&instance, 5, &web3);
    let (_account_state, orders) = reader.get_auction_data(batch.into()).unwrap();

    assert_eq!(orders.len(), 100);
    for (i, order) in orders.iter().enumerate() {
        assert_eq!(order.buy_token as u64, first_token_id);
        assert_eq!(order.sell_token as u64, second_token_id);
        assert_eq!(order.buy_amount, i as u128);
        assert_eq!(order.sell_amount, (i + 1) as u128);
    }
}
