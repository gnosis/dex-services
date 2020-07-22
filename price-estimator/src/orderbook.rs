use anyhow::Result;
use core::{
    models::{AccountState, BatchId, Order},
    orderbook::StableXOrderBookReading,
};
use pricegraph::Pricegraph;
use tokio::sync::RwLock;

#[derive(Debug)]
pub enum PricegraphKind {
    // pricegraph instance with the original orders from the orderbook
    Raw,
    // pricegraph instance with the orders to which the rounding buffer has been applied
    #[allow(dead_code)]
    WithRoundingBuffer,
}

struct PricegraphCache {
    pricegraph_raw: Pricegraph,
    pricegraph_with_rounding_buffer: Pricegraph,
}

/// Access and update the pricegraph orderbook.
pub struct Orderbook {
    orderbook_reading: Box<dyn StableXOrderBookReading>,
    pricegraph_cache: RwLock<PricegraphCache>,
}

impl Orderbook {
    pub fn new(orderbook_reading: Box<dyn StableXOrderBookReading>) -> Self {
        Self {
            orderbook_reading,
            pricegraph_cache: RwLock::new(PricegraphCache {
                pricegraph_raw: Pricegraph::new(std::iter::empty()),
                pricegraph_with_rounding_buffer: Pricegraph::new(std::iter::empty()),
            }),
        }
    }

    pub async fn pricegraph(
        &self,
        batch_id: Option<BatchId>,
        pricegraph_type: PricegraphKind,
    ) -> Result<Pricegraph> {
        match batch_id {
            Some(batch_id) => {
                let mut auction_data = self.auction_data(batch_id).await?;
                if matches!(pricegraph_type, PricegraphKind::WithRoundingBuffer) {
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
        self.pricegraph_cache.write().await.pricegraph_raw = pricegraph;

        self.apply_rounding_buffer_to_auction_data(&mut auction_data)
            .await;
        let pricegraph = pricegraph_from_auction_data(&auction_data);
        self.pricegraph_cache
            .write()
            .await
            .pricegraph_with_rounding_buffer = pricegraph;

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

    async fn cached_pricegraph(&self, pricegraph_type: PricegraphKind) -> Pricegraph {
        let cache = self.pricegraph_cache.read().await;
        match pricegraph_type {
            PricegraphKind::Raw => &cache.pricegraph_raw,
            PricegraphKind::WithRoundingBuffer => &cache.pricegraph_with_rounding_buffer,
        }
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
