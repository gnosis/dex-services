use crate::{
    infallible_price_source::PriceCacheUpdater, models::EstimationTime, solver_rounding_buffer,
};
use anyhow::{bail, Result};
use core::{
    models::{AccountState, BatchId, Order, TokenId},
    orderbook::StableXOrderBookReading,
};
use ethcontract::Address;
use futures::future;
use pricegraph::{Pricegraph, TokenPair};
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
    infallible_price_source: PriceCacheUpdater,
}

impl Orderbook {
    pub fn new(
        orderbook_reading: Box<dyn StableXOrderBookReading>,
        infallible_price_source: PriceCacheUpdater,
        extra_rounding_buffer_factor: f64,
    ) -> Self {
        Self {
            orderbook_reading,
            pricegraph_cache: RwLock::new(PricegraphCache {
                pricegraph_raw: Pricegraph::new(std::iter::empty()),
                pricegraph_with_rounding_buffer: Pricegraph::new(std::iter::empty()),
            }),
            infallible_price_source,
            extra_rounding_buffer_factor,
        }
    }

    pub async fn pricegraph(
        &self,
        time: EstimationTime,
        ignore_addresses: &[Address],
        pricegraph_type: PricegraphKind,
    ) -> Result<Pricegraph> {
        if time == EstimationTime::Now && ignore_addresses.is_empty() {
            Ok(self.cached_pricegraph(pricegraph_type).await)
        } else {
            let mut auction_data = self.auction_data(time).await?;
            if matches!(pricegraph_type, PricegraphKind::WithRoundingBuffer) {
                self.apply_rounding_buffer_to_auction_data(&mut auction_data)
                    .await?;
            }

            Ok(pricegraph_from_auction_data(
                &auction_data,
                ignore_addresses,
            ))
        }
    }

    /// Recreate the pricegraph orderbook and update the infallible price source.
    pub async fn update(&self) -> Result<()> {
        let mut auction_data = self.auction_data(EstimationTime::Now).await?;

        // TODO: Move this cpu heavy computation out of the async function using spawn_blocking.
        let pricegraph = pricegraph_from_auction_data(&auction_data, &[]);
        self.update_infallible_price_source(&pricegraph).await;
        self.pricegraph_cache.write().await.pricegraph_raw = pricegraph;

        self.apply_rounding_buffer_to_auction_data(&mut auction_data)
            .await?;
        let pricegraph = pricegraph_from_auction_data(&auction_data, &[]);
        self.pricegraph_cache
            .write()
            .await
            .pricegraph_with_rounding_buffer = pricegraph;
        Ok(())
    }

    pub async fn rounding_buffer(&self, token_pair: TokenPair) -> f64 {
        let price_source = self.infallible_price_source.inner().await;
        solver_rounding_buffer::rounding_buffer(
            price_source.price(TokenId(0)).get() as f64,
            price_source.price(TokenId(token_pair.sell)).get() as f64,
            price_source.price(TokenId(token_pair.buy)).get() as f64,
            self.extra_rounding_buffer_factor,
        )
    }

    /// Update the infallible price source with the averaged prices of the external price sources
    /// and the pricegraph prices.
    async fn update_infallible_price_source(&self, pricegraph: &Pricegraph) {
        let (token_result, price_result) = future::join(
            self.infallible_price_source.update_tokens(),
            self.infallible_price_source.update_prices(pricegraph),
        )
        .await;
        if let Err(err) = token_result {
            log::error!("failed to update price source tokens: {:?}", err)
        }
        if let Err(err) = price_result {
            log::error!("failed to update price source prices: {:?}", err)
        }
    }

    async fn auction_data(&self, time: EstimationTime) -> Result<AuctionData> {
        match time {
            EstimationTime::Now => {
                self.orderbook_reading
                    .get_auction_data_for_batch(BatchId::now().into())
                    .await
            }
            EstimationTime::Batch(batch_id) => {
                self.orderbook_reading
                    .get_auction_data_for_batch(batch_id.into())
                    .await
            }
            EstimationTime::Block(block_number) => {
                self.orderbook_reading
                    .get_auction_data_for_block(block_number)
                    .await
            }
            EstimationTime::Timestamp(_) => bail!("not yet implemented"),
        }
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
        let price_source = self.infallible_price_source.inner().await;
        let prices = |token_id| price_source.price(token_id);
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

fn pricegraph_from_auction_data(
    auction_data: &AuctionData,
    ignore_addresses: &[Address],
) -> Pricegraph {
    Pricegraph::new(
        auction_data
            .1
            .iter()
            .filter(|order| !ignore_addresses.contains(&order.account_id))
            .map(|order| order.to_element_with_accounts(&auction_data.0)),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::{
        models::TokenId, orderbook::NoopOrderbook, price_estimation::price_source::PriceSource,
        token_info::hardcoded::TokenData,
    };
    use futures::future::{BoxFuture, FutureExt as _};
    use std::{collections::HashMap, num::NonZeroU128, sync::Arc};

    #[test]
    fn updates_infallible_price_source() {
        struct PriceSource_ {};
        impl PriceSource for PriceSource_ {
            fn get_prices<'a>(
                &'a self,
                _tokens: &'a [TokenId],
            ) -> BoxFuture<'a, Result<HashMap<TokenId, NonZeroU128>>> {
                futures::future::ready(Ok(vec![(TokenId(1), NonZeroU128::new(3).unwrap())]
                    .into_iter()
                    .collect()))
                .boxed()
            }
        }

        let token_info = Arc::new(TokenData::default());
        let infallible = PriceCacheUpdater::new(token_info, vec![Box::new(PriceSource_ {})]);
        let orderbook = Orderbook::new(Box::new(NoopOrderbook), infallible, 2.0);
        let price = || {
            orderbook
                .infallible_price_source
                .inner()
                .now_or_never()
                .unwrap()
                .price(TokenId(1))
        };

        let before_update_price = price();
        orderbook.update().now_or_never().unwrap().unwrap();
        let after_update_price = price();
        assert!(before_update_price != after_update_price);
        assert_eq!(after_update_price.get(), 3);
    }

    #[test]
    fn uses_ignored_addresses() {
        let mut account_state = AccountState::default();
        let mut create_order = |address| {
            let amount = 10u128.pow(18);
            account_state.0.insert((address, 0), amount.into());
            Order {
                id: 0,
                account_id: address,
                buy_token: 1,
                sell_token: 0,
                numerator: amount,
                denominator: amount,
                remaining_sell_amount: amount,
                valid_from: 0,
                valid_until: 0,
            }
        };
        let orders = vec![
            create_order(Address::from_low_u64_be(0)),
            create_order(Address::from_low_u64_be(1)),
            create_order(Address::from_low_u64_be(2)),
        ];
        let pricegraph =
            pricegraph_from_auction_data(&(account_state, orders), &[Address::from_low_u64_be(1)]);
        assert_eq!(pricegraph.full_orderbook().num_orders(), 2);
    }
}
