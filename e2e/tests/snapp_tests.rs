use e2e::common::{wait_for, wait_for_condition, FutureBuilderExt};
use e2e::snapp::{await_state_transition, setup_snapp};
use ethcontract::web3::api::Web3;
use ethcontract::web3::transports::Http;
use ethcontract::web3::types::{H160, H256, U128, U256};
use ethcontract::Account;
use std::str::FromStr;

#[test]
fn snapp_deposit_withdraw() {
    let (eloop, http) = Http::new("http://localhost:8545").expect("transport failed");
    eloop.into_remote();
    let web3 = Web3::new(http);
    let (instance, accounts, tokens, db) = setup_snapp(&web3, 3, 3, 100);

    // Test environment values
    let deposit_amount = 18_000_000_000_000_000_000u128;

    let user_address = accounts[2];
    let user_id = instance
        .public_key_to_account_map(user_address)
        .wait_and_expect("Could not recover account id");
    // TODO - Our storage for AccountState should use account_id and NOT H160!
    // This is because AccountState model is shared by StableX and dƒusion
    let db_account_id = H160::from_low_u64_be(user_id);
    // read_balance expects token_id as u16, while ethcontract-rs only accepts u64.
    let token_id = 2u16;
    let initial_balance = tokens[token_id as usize]
        .balance_of(user_address)
        .wait_and_expect("Could not retrieve token balance");

    let initial_state_hash = instance
        .get_current_state_root()
        .wait_and_expect("Could not recover initial state hash");

    instance
        .deposit(token_id.into(), U128::from(deposit_amount))
        .from(Account::Local(user_address, None))
        .wait_and_expect("Failed to send first deposit");

    wait_for(&web3, 181);

    // Check that contract was updated
    let expected_deposit_hash =
        H256::from_str("73815c173218e6025f7cb12d0add44354c4671e261a34a360943007ff6ac7af5").unwrap();

    let after_deposit_state = await_state_transition(&instance, &initial_state_hash);
    assert_eq!(
        expected_deposit_hash,
        H256::from_slice(&after_deposit_state)
    );

    // Check that DB was updated (with correct balances)
    wait_for_condition(|| {
        db.get_balances_for_state_root(&expected_deposit_hash)
            .is_ok()
    })
    .expect("Deposit: Did not detect expected DB update");
    let state = db
        .get_balances_for_state_root(&expected_deposit_hash)
        .unwrap();
    assert_eq!(state.read_balance(token_id, db_account_id), deposit_amount);

    instance
        .request_withdrawal(token_id.into(), U128::from(deposit_amount))
        .from(Account::Local(user_address, None))
        .wait_and_expect("Failed to request withdraw");

    wait_for(&web3, 181);
    let expected_withdraw_hash =
        H256::from_str("7b738197bfe79b6d394499b0cac0186cdc2f65ae2239f2e9e3c698709c80cb67").unwrap();

    let after_withdraw_state = await_state_transition(&instance, &after_deposit_state);
    assert_eq!(
        expected_withdraw_hash,
        H256::from_slice(&after_withdraw_state)
    );

    // Check that DB was updated (with correct balances)
    wait_for_condition(|| {
        db.get_balances_for_state_root(&expected_withdraw_hash)
            .is_ok()
    })
    .expect("Withdraw: Did not detect expected DB update");
    let state = db
        .get_balances_for_state_root(&expected_withdraw_hash)
        .unwrap();

    assert_eq!(state.read_balance(token_id, db_account_id), 0);

    // TODO - Construct Merkle proof from state of accounts.
    let merkle_proof = [
        0x00u8, 0x00u8, 0x00u8, 0x00u8, 0x00u8, 0x00u8, 0x00u8, 0x00u8, 0x00u8, 0x00u8, 0x00u8,
        0x00u8, 0x00u8, 0x00u8, 0x00u8, 0x00u8, 0x00u8, 0x00u8, 0x00u8, 0x00u8, 0x00u8, 0x00u8,
        0x00u8, 0x00u8, 0x00u8, 0x00u8, 0x00u8, 0x00u8, 0x00u8, 0x00u8, 0x00u8, 0x00u8, 0xf5u8,
        0xa5u8, 0xfdu8, 0x42u8, 0xd1u8, 0x6au8, 0x20u8, 0x30u8, 0x27u8, 0x98u8, 0xefu8, 0x6eu8,
        0xd3u8, 0x09u8, 0x97u8, 0x9bu8, 0x43u8, 0x00u8, 0x3du8, 0x23u8, 0x20u8, 0xd9u8, 0xf0u8,
        0xe8u8, 0xeau8, 0x98u8, 0x31u8, 0xa9u8, 0x27u8, 0x59u8, 0xfbu8, 0x4bu8, 0xdbu8, 0x56u8,
        0x11u8, 0x4eu8, 0x00u8, 0xfdu8, 0xd4u8, 0xc1u8, 0xf8u8, 0x5cu8, 0x89u8, 0x2bu8, 0xf3u8,
        0x5au8, 0xc9u8, 0xa8u8, 0x92u8, 0x89u8, 0xaau8, 0xecu8, 0xb1u8, 0xebu8, 0xd0u8, 0xa9u8,
        0x6cu8, 0xdeu8, 0x60u8, 0x6au8, 0x74u8, 0x8bu8, 0x5du8, 0x71u8, 0xc7u8, 0x80u8, 0x09u8,
        0xfdu8, 0xf0u8, 0x7fu8, 0xc5u8, 0x6au8, 0x11u8, 0xf1u8, 0x22u8, 0x37u8, 0x06u8, 0x58u8,
        0xa3u8, 0x53u8, 0xaau8, 0xa5u8, 0x42u8, 0xedu8, 0x63u8, 0xe4u8, 0x4cu8, 0x4bu8, 0xc1u8,
        0x5fu8, 0xf4u8, 0xcdu8, 0x10u8, 0x5au8, 0xb3u8, 0x3cu8, 0x53u8, 0x6du8, 0x98u8, 0x83u8,
        0x7fu8, 0x2du8, 0xd1u8, 0x65u8, 0xa5u8, 0x5du8, 0x5eu8, 0xeau8, 0xe9u8, 0x14u8, 0x85u8,
        0x95u8, 0x44u8, 0x72u8, 0xd5u8, 0x6fu8, 0x24u8, 0x6du8, 0xf2u8, 0x56u8, 0xbfu8, 0x3cu8,
        0xaeu8, 0x19u8, 0x35u8, 0x2au8, 0x12u8, 0x3cu8, 0x9eu8, 0xfdu8, 0xe0u8, 0x52u8, 0xaau8,
        0x15u8, 0x42u8, 0x9fu8, 0xaeu8, 0x05u8, 0xbau8, 0xd4u8, 0xd0u8, 0xb1u8, 0xd7u8, 0xc6u8,
        0x4du8, 0xa6u8, 0x4du8, 0x03u8, 0xd7u8, 0xa1u8, 0x85u8, 0x4au8, 0x58u8, 0x8cu8, 0x2cu8,
        0xb8u8, 0x43u8, 0x0cu8, 0x0du8, 0x30u8, 0xd8u8, 0x8du8, 0xdfu8, 0xeeu8, 0xd4u8, 0x00u8,
        0xa8u8, 0x75u8, 0x55u8, 0x96u8, 0xb2u8, 0x19u8, 0x42u8, 0xc1u8, 0x49u8, 0x7eu8, 0x11u8,
        0x4cu8, 0x30u8, 0x2eu8, 0x61u8, 0x18u8, 0x29u8, 0x0fu8, 0x91u8, 0xe6u8, 0x77u8, 0x29u8,
        0x76u8, 0x04u8, 0x1fu8, 0xa1u8,
    ];

    // Claim withdraw
    instance
        .claim_withdrawal(
            U256::zero(),
            0,
            user_id,
            token_id.into(),
            U128::from(deposit_amount),
            merkle_proof.to_vec(),
        )
        .from(Account::Local(user_address, None))
        .wait_and_expect("Failed to claim withdraw");

    let final_balance = tokens[token_id as usize]
        .balance_of(user_address)
        .wait_and_expect("Could not retrieve token balance");
    assert_eq!(final_balance, initial_balance);
}

#[test]
fn snapp_auction() {
    let (eloop, http) = Http::new("http://localhost:8545").expect("transport failed");
    eloop.into_remote();
    let web3 = Web3::new(http);
    let (instance, accounts, _tokens, db) = setup_snapp(&web3, 3, 6, 300);

    // Test environment values
    let deposit_amount = 300_000_000_000_000_000_000u128;

    let initial_state_hash = instance
        .get_current_state_root()
        .wait_and_expect("Could not recover initial state hash");

    println!("Depositing sufficient funds for trades");
    let deposit_tokens = [2u64, 1, 2, 1, 0, 0];
    for (account, token_id) in accounts.iter().zip(deposit_tokens.iter()) {
        println!(
            "    deposit(tokenId={}, amount={}, {{ from: {} }})",
            token_id, deposit_amount, account
        );
        instance
            .deposit(*token_id, U128::from(deposit_amount))
            .from(Account::Local(*account, None))
            .wait_and_expect("Failed to send deposit");
    }

    wait_for(&web3, 181);
    let post_deposit_state = await_state_transition(&instance, &initial_state_hash);
    println!(
        "After Deposit State {:?}",
        H256::from_slice(&post_deposit_state)
    );

    println!("Placing 6 orders in current auction");
    let buy_tokens = [1u64, 2, 0, 0, 1, 2];
    let buy_sell_tokens = buy_tokens.iter().zip(deposit_tokens.iter());
    let buy_sell_amounts = [
        (
            12_000_000_000_000_000_000u128,
            12_000_000_000_000_000_000u128,
        ),
        (2_200_000_000_000_000_000u128, 2_000_000_000_000_000_000u128),
        (
            150_000_000_000_000_000_000u128,
            10_000_000_000_000_000_000u128,
        ),
        (
            180_000_000_000_000_000_000u128,
            15_000_000_000_000_000_000u128,
        ),
        (
            4_000_000_000_000_000_000u128,
            52_000_000_000_000_000_000u128,
        ),
        (
            20_000_000_000_000_000_000u128,
            280_000_000_000_000_000_000u128,
        ),
    ];
    for (account, ((buy_token, sell_token), (buy_amount, sell_amount))) in accounts
        .iter()
        .zip(buy_sell_tokens.zip(buy_sell_amounts.iter()))
    {
        println!(
            "    placeOrder(buyToken={}, sellToken={}, buyAmount={}, sellAmount={}, {{ from: {} }})",
            buy_token, sell_token, buy_amount, sell_amount, account
        );
        instance
            .place_sell_order(
                *buy_token,
                *sell_token,
                U128::from(*buy_amount),
                U128::from(*sell_amount),
            )
            .from(Account::Local(*account, None))
            .wait_and_expect("Could not place order");
    }

    println!("Awaiting order inclusion in DB");
    wait_for_condition(|| db.get_orders_of_slot(&U256::zero()).is_ok())
        .expect("Did not detect order inclusion in DB");
    wait_for_condition(|| db.get_orders_of_slot(&U256::zero()).unwrap().len() == 6)
        .expect("Could not fetch all orders");
    let orders = db.get_orders_of_slot(&U256::zero()).unwrap();
    assert_eq!(orders[5].sell_amount, buy_sell_amounts[5].1);

    println!("Advance time to bid for auction");
    wait_for(&web3, 181);

    //    let expected_state_hash =
    //        H256::from_str("572dd059c22fe72a966510cba30961215c9e60b96359ccb79996ad3f9c1668f8").unwrap();
    // TODO - check auction state on contract
    let bid_hash = instance
        .auctions(U256::zero())
        .wait_and_expect("No auction bid detected on smart contract");
    println!("Pending Auction Bid {:#?}", bid_hash);

    println!("Advance time for auction settlement");
    wait_for(&web3, 181);
    //    let post_auction_state = await_state_transition(&instance, &post_deposit_state);
    //    assert_eq!(expected_state_hash, H256::from_slice(&post_auction_state));

    //    let state = db
    //        .get_balances_for_state_root(&H256::from_slice(&bid_hash))
    //        .unwrap();
    //
    //    assert_eq!(
    //        state.read_balance(1, H160::from_low_u64_be(4 + 1)),
    //        4_000_000_000_000_000_000u128,
    //        "Account 4 should now have 4 of token 1"
    //    );
    //    assert_eq!(
    //        state.read_balance(0, H160::from_low_u64_be(3 + 1)),
    //        52_000_000_000_000_000_000u128,
    //        "Account 3 should now have 52 of token 0"
    //    );
}
