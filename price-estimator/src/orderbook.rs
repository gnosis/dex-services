use core::orderbook::StableXOrderBookReading;
use pricegraph::Pricegraph;
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
                .map(|order| order.to_element_with_accounts(&account_state)),
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
