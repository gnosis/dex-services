use anyhow::Result;
use core::{
    models::{AccountState, BatchId, Order},
    orderbook::StableXOrderBookReading,
};
use pricegraph::Pricegraph;
use tokio::sync::RwLock;

#[derive(Debug)]
pub enum PricegraphType {
    // pricegraph instance with the original orders from the orderbook
    Raw,
    // pricegraph instance with the orders to which the rounding buffer has been applied
    #[allow(dead_code)]
    WithRoundingBuffer,
}

/// Access and update the pricegraph orderbook.
pub struct Orderbook {
    orderbook_reading: Box<dyn StableXOrderBookReading>,
    pricegraph_raw: RwLock<Pricegraph>,
    pricegraph_with_rounding_buffer: RwLock<Pricegraph>,
}

impl Orderbook {
    pub fn new(orderbook_reading: Box<dyn StableXOrderBookReading>) -> Self {
        Self {
            orderbook_reading,
            pricegraph_raw: RwLock::new(Pricegraph::new(std::iter::empty())),
            pricegraph_with_rounding_buffer: RwLock::new(Pricegraph::new(std::iter::empty())),
        }
    }

    pub async fn pricegraph(
        &self,
        batch_id: Option<BatchId>,
        pricegraph_type: PricegraphType,
    ) -> Result<Pricegraph> {
        match batch_id {
            Some(batch_id) => {
                let mut auction_data = self.auction_data(batch_id).await?;
                if matches!(pricegraph_type, PricegraphType::WithRoundingBuffer) {
                    self.apply_rounding_buffer_to_auction_data(&mut auction_data)
                        .await;
                }
                Ok(pricegraph_from_auction_data(&auction_data))
            }
            None => Ok(self.cached_pricegraph(pricegraph_type).await),
        }
    }

    /// Recreate the pricegraph orderbook.
    pub async fn update(&self) -> Result<()> {
        let mut auction_data = self.auction_data(BatchId::now()).await?;

        // TODO: Move this cpu heavy computation out of the async function using spawn_blocking.
        let pricegraph = pricegraph_from_auction_data(&auction_data);
        *self.pricegraph_raw.write().await = pricegraph;

        self.apply_rounding_buffer_to_auction_data(&mut auction_data)
            .await;
        let pricegraph = pricegraph_from_auction_data(&auction_data);
        *self.pricegraph_with_rounding_buffer.write().await = pricegraph;

        Ok(())
    }

    async fn auction_data(&self, batch_id: BatchId) -> Result<AuctionData> {
        self.orderbook_reading
            .get_auction_data(batch_id.into())
            .await
    }

    async fn apply_rounding_buffer_to_auction_data(&self, _auction_data: &mut AuctionData) {
        // TODO
    }

    async fn cached_pricegraph(&self, pricegraph_type: PricegraphType) -> Pricegraph {
        match pricegraph_type {
            PricegraphType::Raw => &self.pricegraph_raw,
            PricegraphType::WithRoundingBuffer => &self.pricegraph_with_rounding_buffer,
        }
        .read()
        .await
        .clone()
    }
}

type AuctionData = (AccountState, Vec<Order>);

fn pricegraph_from_auction_data(auction_data: &AuctionData) -> Pricegraph {
    Pricegraph::new(
        auction_data
            .1
            .iter()
            .map(|order| order.to_element_with_accounts(&auction_data.0)),
    )
}
