use core::{
    models::{AccountState, Order},
    orderbook::StableXOrderBookReading,
};
use pricegraph::{Element, Price, Pricegraph, TokenPair, Validity};
use std::time::{Duration, SystemTime};
use tokio::sync::RwLock;

/// Access and update the pricegraph orderbook.
pub struct Orderbook<T> {
    orderbook_reading: T,
    pricegraph: RwLock<Pricegraph>,
}

impl<T> Orderbook<T> {
    pub fn new(orderbook_reading: T) -> Self {
        Self {
            orderbook_reading,
            pricegraph: RwLock::new(Pricegraph::new(std::iter::empty())),
        }
    }

    pub async fn get_pricegraph(&self) -> Pricegraph {
        self.pricegraph.read().await.clone()
    }
}

impl<T: StableXOrderBookReading> Orderbook<T> {
    /// Recreate the pricegraph orderbook.
    pub async fn update(&self) -> anyhow::Result<()> {
        let (account_state, orders) = self
            .orderbook_reading
            .get_auction_data(current_batch_id() as u32)
            .await?;

        // TODO: Move this cpu heavy computation out of the async function using spawn_blocking.
        let pricegraph = Pricegraph::new(
            orders
                .iter()
                .map(|order| order_to_element(order, &account_state)),
        );

        *self.pricegraph.write().await = pricegraph;
        Ok(())
    }
}

/// Current batch id based on system time.
fn current_batch_id() -> u64 {
    const BATCH_DURATION: Duration = Duration::from_secs(300);
    let now = SystemTime::now();
    let time_since_epoch = now
        .duration_since(SystemTime::UNIX_EPOCH)
        .expect("unix epoch is not in the past");
    time_since_epoch.as_secs() / BATCH_DURATION.as_secs()
}

/// Convert a core Order to a pricegraph Element.
fn order_to_element(order: &Order, account_state: &AccountState) -> Element {
    // Some conversions are needed because the primitive types crate is on different versions in
    // core and pricegraph.
    Element {
        user: order.account_id,
        balance: account_state.read_balance(order.sell_token, order.account_id),
        pair: TokenPair {
            buy: order.buy_token,
            sell: order.sell_token,
        },
        valid: Validity {
            from: order.valid_from,
            to: order.valid_until,
        },
        price: Price {
            numerator: order.numerator,
            denominator: order.denominator,
        },
        remaining_sell_amount: order.remaining_sell_amount,
        id: order.id,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ethcontract::{Address, U256};
    use std::collections::HashMap;

    #[test]
    fn order_to_element_() {
        let order = Order {
            id: 1,
            account_id: Address::from_low_u64_le(2),
            buy_token: 3,
            sell_token: 4,
            numerator: 5,
            denominator: 6,
            remaining_sell_amount: 7,
            valid_from: 8,
            valid_until: 9,
        };
        let mut account_state = AccountState(HashMap::new());
        account_state
            .0
            .insert((order.account_id, order.sell_token), U256::from(10));
        let element = order_to_element(&order, &account_state);
        assert_eq!(
            element.user.as_fixed_bytes(),
            order.account_id.as_fixed_bytes()
        );
        assert_eq!(element.balance.0, U256::from(10).0);
        assert_eq!(element.pair.buy, order.buy_token);
        assert_eq!(element.pair.sell, order.sell_token);
        assert_eq!(element.valid.from, order.valid_from);
        assert_eq!(element.valid.to, order.valid_until);
        assert_eq!(element.price.numerator, order.numerator);
        assert_eq!(element.price.denominator, order.denominator);
        assert_eq!(element.remaining_sell_amount, order.remaining_sell_amount);
        assert_eq!(element.id, order.id);
    }
}
