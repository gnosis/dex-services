use crate::models::{AccountState, Order};
use ethcontract::Address;

/// Removes empty orders and token balances for which there is
/// not at least one sell order by that user
pub fn normalize_auction_data(
    account_states: impl IntoIterator<Item = ((Address, u16), u128)>,
    orders: impl IntoIterator<Item = Order>,
) -> (AccountState, Vec<Order>) {
    let orders = orders
        .into_iter()
        .filter(|order| order.sell_amount > 0)
        .collect::<Vec<_>>();
    let account_states = account_states
        .into_iter()
        .filter(|((user, token), _)| {
            orders
                .iter()
                .any(|order| order.account_id == *user && order.sell_token == *token)
        })
        .map(|(key, value)| (key, value))
        .collect();
    (AccountState(account_states), orders)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_filter_account_state() {
        let orders = vec![
            Order {
                id: 0,
                account_id: Address::zero(),
                buy_token: 0,
                sell_token: 1,
                buy_amount: 1,
                sell_amount: 1,
            },
            Order {
                id: 0,
                account_id: Address::repeat_byte(1),
                buy_token: 0,
                sell_token: 2,
                buy_amount: 0,
                sell_amount: 0,
            },
        ];
        let account_states = vec![
            ((Address::zero(), 0), 3),
            ((Address::zero(), 1), 4),
            ((Address::zero(), 2), 5),
        ];

        let (account_state, orders) = normalize_auction_data(account_states, orders);
        assert_eq!(account_state.0.len(), 1);
        assert_eq!(account_state.read_balance(1, Address::zero()), 4);
        assert_eq!(orders.len(), 1);
        assert_eq!(orders[0].account_id, Address::zero());
    }
}
