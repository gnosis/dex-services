//! Module for historic orderbook reading.

use super::{NoopOrderbook, StableXOrderBookReading};
use crate::models::{AccountState, Order};
use anyhow::Result;
use ethcontract::BlockNumber;
use futures::future::BoxFuture;
use std::time::SystemTime;

/// Trait for reading orderbook data by block or timestamp.
pub trait OrderbookReadingByBlockOrTimestamp: StableXOrderBookReading {
    /// Retrieves the open orderbook at the specified block number.
    ///
    /// The open orderbook is defined as the auction state for the current batch
    /// at the specified block number. This implies that, assuming no futher
    /// changes to the exchange, this will be the auction state passed to the
    /// solver to be submitted in the next batch.
    fn get_auction_data_for_block<'a>(
        &'a self,
        block_number: BlockNumber,
    ) -> BoxFuture<'a, Result<(AccountState, Vec<Order>)>>;

    /// Retrieves the open orderbook at the specified timestamp.
    fn get_auction_data_for_timestamp<'a>(
        &'a self,
        timestamp: SystemTime,
    ) -> BoxFuture<'a, Result<(AccountState, Vec<Order>)>>;
}

impl OrderbookReadingByBlockOrTimestamp for NoopOrderbook {
    fn get_auction_data_for_block<'a>(
        &'a self,
        _: BlockNumber,
    ) -> BoxFuture<'a, Result<(AccountState, Vec<Order>)>> {
        immediate!(Ok(Default::default()))
    }

    fn get_auction_data_for_timestamp<'a>(
        &'a self,
        _: SystemTime,
    ) -> BoxFuture<'a, Result<(AccountState, Vec<Order>)>> {
        immediate!(Ok(Default::default()))
    }
}

#[cfg(test)]
mod mock {
    use super::*;

    /// Proxy trait used for mocking a `OrderbookReadingByBlockOrTimestamp` to
    /// work around the fact that `mockall` doesn't support trait inheritance.
    #[mockall::automock]
    pub trait Proxy {
        fn get_auction_data<'a>(
            &'a self,
            batch_id_to_solve: u32,
        ) -> BoxFuture<'a, Result<(AccountState, Vec<Order>)>>;
        fn initialize<'a>(&'a self) -> BoxFuture<'a, Result<()>>;
        fn get_auction_data_for_block<'a>(
            &'a self,
            block_number: BlockNumber,
        ) -> BoxFuture<'a, Result<(AccountState, Vec<Order>)>>;
        fn get_auction_data_for_timestamp<'a>(
            &'a self,
            timestamp: SystemTime,
        ) -> BoxFuture<'a, Result<(AccountState, Vec<Order>)>>;
    }

    impl StableXOrderBookReading for MockProxy {
        fn get_auction_data<'a>(
            &'a self,
            batch_id_to_solve: u32,
        ) -> BoxFuture<'a, Result<(AccountState, Vec<Order>)>> {
            Proxy::get_auction_data(self, batch_id_to_solve)
        }
        fn initialize<'a>(&'a self) -> BoxFuture<'a, Result<()>> {
            Proxy::initialize(self)
        }
    }

    impl OrderbookReadingByBlockOrTimestamp for MockProxy {
        fn get_auction_data_for_block<'a>(
            &'a self,
            block_number: BlockNumber,
        ) -> BoxFuture<'a, Result<(AccountState, Vec<Order>)>> {
            Proxy::get_auction_data_for_block(self, block_number)
        }
        fn get_auction_data_for_timestamp<'a>(
            &'a self,
            timestamp: SystemTime,
        ) -> BoxFuture<'a, Result<(AccountState, Vec<Order>)>> {
            Proxy::get_auction_data_for_timestamp(self, timestamp)
        }
    }
}

#[cfg(test)]
pub use mock::MockProxy as MockOrderbookReadingByBlockOrTimestamp;
