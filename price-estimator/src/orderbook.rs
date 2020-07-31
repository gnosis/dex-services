use crate::{infallible_price_source::InfalliblePriceSource, solver_rounding_buffer};
use anyhow::Result;
use core::{
    models::{AccountState, BatchId, Order, TokenId},
    orderbook::StableXOrderBookReading,
};
use pricegraph::{Pricegraph, TokenPair};
use std::collections::HashMap;
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
    extra_rounding_buffer_factor: f64,
    infallible_price_source: InfalliblePriceSource,
}

impl Orderbook {
    pub fn new(orderbook_reading: Box<dyn StableXOrderBookReading>) -> Self {
        Self {
            orderbook_reading,
            pricegraph_cache: RwLock::new(PricegraphCache {
                pricegraph_raw: Pricegraph::new(std::iter::empty()),
                pricegraph_with_rounding_buffer: Pricegraph::new(std::iter::empty()),
            }),
            // TODO: pass this from command line argument
            extra_rounding_buffer_factor: 2.0,
            // TODO: pass real token infos, and update it with a real price source in a future PR
            infallible_price_source: InfalliblePriceSource::new(HashMap::new()),
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
                        .await?;
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
            .await?;
        let pricegraph = pricegraph_from_auction_data(&auction_data);
        self.pricegraph_cache
            .write()
            .await
            .pricegraph_with_rounding_buffer = pricegraph;
        Ok(())
    }

    // TODO: this function is going to be used in some routes
    pub async fn _rounding_buffer(&self, token_pair: TokenPair) -> Result<f64> {
        Ok(solver_rounding_buffer::rounding_buffer(
            self.infallible_price_source.price(TokenId(0)) as f64,
            self.infallible_price_source.price(TokenId(token_pair.sell)) as f64,
            self.infallible_price_source.price(TokenId(token_pair.buy)) as f64,
            self.extra_rounding_buffer_factor,
        ))
    }

    async fn auction_data(&self, batch_id: BatchId) -> Result<AuctionData> {
        self.orderbook_reading
            .get_auction_data(batch_id.into())
            .await
    }

    async fn cached_pricegraph(&self, pricegraph_type: PricegraphKind) -> Pricegraph {
        let cache = self.pricegraph_cache.read().await;
        match pricegraph_type {
            PricegraphKind::Raw => &cache.pricegraph_raw,
            PricegraphKind::WithRoundingBuffer => &cache.pricegraph_with_rounding_buffer,
        }
        .clone()
    }

    async fn apply_rounding_buffer_to_auction_data(
        &self,
        auction_data: &mut AuctionData,
    ) -> Result<()> {
        let prices = |token_id| self.infallible_price_source.price(token_id);
        solver_rounding_buffer::apply_rounding_buffer(
            prices,
            &mut auction_data.1,
            &mut auction_data.0,
            self.extra_rounding_buffer_factor,
        );
        Ok(())
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
