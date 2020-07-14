use crate::solver_rounding_buffer;
use core::{
    models::TokenId, orderbook::StableXOrderBookReading,
    price_estimation::price_source::PriceSource,
};
use pricegraph::{Pricegraph, TokenPair};
use std::{
    sync::Arc,
    time::{Duration, SystemTime},
};
use tokio::sync::RwLock;

pub type DynPriceSource = Box<dyn PriceSource + Send + Sync>;

/// Access and update the pricegraph orderbook.
pub struct Orderbook {
    orderbook_reading: Arc<dyn StableXOrderBookReading>,
    price_source: DynPriceSource,
    all_tokens: Vec<TokenId>,
    pricegraph: RwLock<Pricegraph>,
    rounding_buffer_factor: f64,
}

impl Orderbook {
    pub fn new(
        orderbook_reading: Arc<dyn StableXOrderBookReading>,
        price_source: DynPriceSource,
        all_tokens: Vec<TokenId>,
        rounding_buffer_factor: f64,
    ) -> Self {
        assert!(rounding_buffer_factor.is_finite());
        assert!(rounding_buffer_factor >= 0.0);
        Self {
            orderbook_reading,
            price_source,
            all_tokens,
            pricegraph: RwLock::new(Pricegraph::new(std::iter::empty())),
            rounding_buffer_factor,
        }
    }

    pub async fn pricegraph(&self) -> Pricegraph {
        self.pricegraph.read().await.clone()
    }

    /// See solver_rounding_buffer::rounding_buffer. This is used to adjust amounts in api queries
    /// to adapt to how the solver sees them.
    /// If any price is not available `1` is used as a fallback.
    pub async fn rounding_buffer(&self, token_pair: TokenPair) -> f64 {
        let fee_token = TokenId(0);
        let sell_token = TokenId(token_pair.sell);
        let buy_token = TokenId(token_pair.buy);
        let prices = self
            .price_source
            .get_prices(&[fee_token, sell_token, buy_token])
            .await
            .unwrap_or_default();
        let get_price = |token_id| prices.get(&token_id).copied().unwrap_or(1) as f64;
        solver_rounding_buffer::rounding_buffer(
            get_price(fee_token),
            get_price(sell_token),
            get_price(buy_token),
        ) * self.rounding_buffer_factor
    }

    /// Recreate the pricegraph orderbook.
    pub async fn update(&self) -> anyhow::Result<()> {
        let (mut account_state, mut orders) = self
            .orderbook_reading
            .get_auction_data(current_batch_id() as u32)
            .await?;

        let token_prices = self.price_source.get_prices(&self.all_tokens).await?;
        solver_rounding_buffer::apply_rounding_buffer(
            &token_prices,
            &mut orders,
            &mut account_state,
            self.rounding_buffer_factor,
        );

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
