use anyhow::Result;
use core::{models::BatchId, orderbook::StableXOrderBookReading};
use pricegraph::Pricegraph;
use tokio::sync::RwLock;

/// Access and update the pricegraph orderbook.
pub struct Orderbook {
    orderbook_reading: Box<dyn StableXOrderBookReading>,
    pricegraph: RwLock<Pricegraph>,
}

impl Orderbook {
    pub fn new(orderbook_reading: Box<dyn StableXOrderBookReading>) -> Self {
        Self {
            orderbook_reading,
            pricegraph: RwLock::new(Pricegraph::new(std::iter::empty())),
        }
    }

    /// Creates a `Pricegraph` instance at the given batch ID.
    async fn create_pricegraph_at_batch(&self, batch_id: BatchId) -> Result<Pricegraph> {
        let (account_state, orders) = self
            .orderbook_reading
            .get_auction_data(batch_id.into())
            .await?;

        // TODO: Move this cpu heavy computation out of the async function using spawn_blocking.
        let pricegraph = Pricegraph::new(
            orders
                .iter()
                .map(|order| order.to_element_with_accounts(&account_state)),
        );

        Ok(pricegraph)
    }

    pub async fn get_pricegraph(&self, batch_id: Option<BatchId>) -> Result<Pricegraph> {
        match batch_id {
            Some(batch_id) => self.create_pricegraph_at_batch(batch_id).await,
            None => Ok(self.pricegraph.read().await.clone()),
        }
    }

    /// Recreate the pricegraph orderbook.
    pub async fn update(&self) -> Result<()> {
        let pricegraph = self.create_pricegraph_at_batch(BatchId::now()).await?;
        *self.pricegraph.write().await = pricegraph;
        Ok(())
    }
}
