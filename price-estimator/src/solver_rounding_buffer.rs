use core::models::{AccountState, Order, TokenId, TokenInfo};
use ethcontract::{Address, U256};
use std::collections::{BTreeMap, HashMap};

// This code is closely related to dex-solver/src/opt/process/Rounding.py .
// Discussion of motivation happened in https://github.com/gnosis/dex-services/issues/970 .

type Tokens = BTreeMap<TokenId, TokenInfo>;

const MAX_ROUNDING_VOLUME: f64 = 100_000_000_000.0;
const PRICE_ESTIMATION_ERROR: f64 = 10.0;

fn max_rounding_amount(token_price: f64, fee_token_price: f64) -> f64 {
    let estimated_price_in_fee_token = token_price / fee_token_price;
    let max_rounding_amount = MAX_ROUNDING_VOLUME / estimated_price_in_fee_token;
    max_rounding_amount.max(1.0)
}

/// Perform the same rounding buffer calculation as our solvers in order to increase the correctness
/// of our estimates.
/// All token prices must be nonzero.
#[allow(dead_code)]
pub fn apply_rounding_buffer(
    tokens: &Tokens,
    orders: &mut Vec<Order>,
    account_state: &mut AccountState,
    pessimistic_factor: f64,
) {
    let fee_token_price = tokens
        .get(&TokenId(0))
        .expect("fee token does not have price")
        .external_price as f64;
    assert!(fee_token_price > 0.0, "fee token price is 0");

    // The maximum rounding buffer over all orders from this address selling this token.
    let mut account_balance_buffers = HashMap::<(Address, TokenId), u128>::new();
    // Apply rounding buffer to account balances and order sell amounts.
    for order in orders.iter_mut() {
        // Compute rounding buffer.
        // If a token doesn't have a price it means it isn't connected to the fee. In this case we
        // use a price of 1 to get some reasonable default rounding buffer.
        let (sell_token, buy_token) = (TokenId(order.sell_token), TokenId(order.buy_token));
        let estimated_buy_token_price = match tokens.get(&buy_token) {
            Some(TokenInfo { external_price, .. }) if *external_price > 0 => *external_price as f64,
            _ => 1.0,
        };
        let estimated_sell_token_price = match tokens.get(&sell_token) {
            Some(TokenInfo { external_price, .. }) if *external_price > 0 => *external_price as f64,
            _ => 1.0,
        };
        let estimated_xrate = estimated_buy_token_price / estimated_sell_token_price;
        let rounding_buffer = max_rounding_amount(estimated_buy_token_price, fee_token_price)
            * estimated_xrate
            * PRICE_ESTIMATION_ERROR.powi(2);
        // Multiply by an extra factor which the solver doesn't do, as added safety in case the
        // prices move.
        let rounding_buffer = (pessimistic_factor * rounding_buffer) as u128;

        // Update rounding buffer for account balances;
        let entry = account_balance_buffers
            .entry((order.account_id, sell_token))
            .or_default();
        *entry = (*entry).max(rounding_buffer);

        // Reduce order sell amount.
        order.denominator = order.denominator.saturating_sub(rounding_buffer);
        order.remaining_sell_amount = order.remaining_sell_amount.saturating_sub(rounding_buffer);
    }

    // Reduce account balances.
    for ((address, token_id), rounding_buffer) in account_balance_buffers {
        if let Some(balance) = account_state.0.get_mut(&(address, token_id.0)) {
            *balance = balance.saturating_sub(U256::from(rounding_buffer));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ethcontract::{Address, U256};

    fn address(n: u64) -> Address {
        Address::from_low_u64_le(n)
    }

    fn account(address_: u64, token: u16, balance: u128) -> ((Address, u16), U256) {
        ((address(address_), token), U256::from(balance))
    }

    fn token_info(external_price: u128) -> TokenInfo {
        TokenInfo {
            alias: None,
            decimals: None,
            external_price,
        }
    }

    fn order(
        id: u16,
        address_: u64,
        buy_token: u16,
        sell_token: u16,
        numerator: u128,
        denominator: u128,
    ) -> Order {
        Order {
            id,
            account_id: address(address_),
            buy_token,
            sell_token,
            numerator,
            denominator,
            remaining_sell_amount: denominator,
            valid_from: 0,
            valid_until: 0,
        }
    }

    #[test]
    fn apply_rounding_buffer_ok() {
        let mut tokens = Tokens::new();
        tokens.insert(TokenId(0), token_info(1));
        tokens.insert(TokenId(1), token_info(2));
        tokens.insert(TokenId(2), token_info(10));

        let accounts = vec![
            account(0, 0, 100_000_000_000_000_000),
            account(0, 1, 100_000_000_000_000_000),
            account(0, 2, 100_000_000_000_000_000),
        ];
        let mut account_state = AccountState(accounts.into_iter().collect());

        let mut orders = vec![
            order(0, 0, 1, 0, 600_000_000_000_000, 500_000_000_000_000),
            order(1, 0, 0, 1, 600_000_000_000_000, 500_000_000_000_000),
            order(2, 0, 2, 0, 600_000_000_000_000, 500_000_000_000_000),
            order(3, 0, 0, 2, 600_000_000_000_000, 500_000_000_000_000),
            order(4, 0, 2, 1, 600_000_000_000_000, 500_000_000_000_000),
            order(5, 0, 1, 2, 600_000_000_000_000, 500_000_000_000_000),
        ];

        apply_rounding_buffer(&tokens, &mut orders, &mut account_state, 1.0);

        let expected_orders = vec![
            order(0, 0, 1, 0, 600_000_000_000_000, 490_000_000_000_000),
            order(1, 0, 0, 1, 600_000_000_000_000, 495_000_000_000_000),
            order(2, 0, 2, 0, 600_000_000_000_000, 490_000_000_000_000),
            order(3, 0, 0, 2, 600_000_000_000_000, 499_000_000_000_000),
            order(4, 0, 2, 1, 600_000_000_000_000, 495_000_000_000_000),
            order(5, 0, 1, 2, 600_000_000_000_000, 499_000_000_000_000),
        ];
        for (order, expected) in orders.iter().zip(expected_orders.iter()) {
            assert_eq!(order, expected);
        }

        let expected_accounts = vec![
            account(0, 0, 99_990_000_000_000_000),
            account(0, 1, 99_995_000_000_000_000),
            account(0, 2, 99_999_000_000_000_000),
        ];
        assert_eq!(
            account_state,
            AccountState(expected_accounts.into_iter().collect())
        );
    }
}
