//! Implementation of an EVM-based scheduler that retrieves current batch and
//! batch duration information directly from the EVM instead of system time.

use super::{AuctionTimingConfiguration, Scheduler};
use crate::contracts::stablex_contract::StableXContract;
use crate::driver::stablex_driver::{DriverResult, StableXDriver};
use crate::models::batch_id::BATCH_DURATION;
use crate::util::FutureWaitExt as _;
use anyhow::Result;
use log::{debug, error, info, warn};
use std::thread;
use std::time::Duration;

/// The amount of time the scheduler should wait between polling.
const POLL_TIMEOUT: Duration = Duration::from_secs(5);

/// An EVM-based scheduler for the exchange driver.
pub struct EvmScheduler<'a> {
    exchange: &'a dyn StableXContract,
    driver: &'a dyn StableXDriver,
    config: AuctionTimingConfiguration,
    last_batch: Option<u32>,
}

impl<'a> EvmScheduler<'a> {
    /// Creates a new EVM-based scheduler from a configuration.
    pub fn new(
        exchange: &'a dyn StableXContract,
        driver: &'a dyn StableXDriver,
        config: AuctionTimingConfiguration,
    ) -> Self {
        EvmScheduler {
            driver,
            exchange,
            config,
            last_batch: None,
        }
    }

    /// Creates a new scheduler with the default configuration.
    #[cfg(test)]
    pub fn with_defaults(exchange: &'a dyn StableXContract, driver: &'a dyn StableXDriver) -> Self {
        EvmScheduler::new(exchange, driver, AuctionTimingConfiguration::default())
    }

    /// Runs the scheduler for a single iteration.
    fn step(&mut self) -> Result<()> {
        let batch_id = self.current_solving_batch()?;
        if self
            .last_batch
            .map(|last_batch| batch_id <= last_batch)
            .unwrap_or_default()
        {
            debug!("skipping already processed batch {}", batch_id);
            return Ok(());
        }

        let time_remaining = self.exchange.get_current_auction_remaining_time().wait()?;
        // NOTE: We need to take into account the asynchronous nature of web3
        //   and handle the case where we query the batch information right on
        //   a batch border and the following happens:
        //     - query batch ID and get `N`
        //     - block gets mined, batch ID becomes `N+1`
        //     - query time remaining and get around `300s`
        //   In order to work around this, we just re-query the batch ID after
        //   getting the time in the batch to make sure we are using the correct
        //   batch. If they don't match, we just return to restart the run-loop.
        let verify_batch_id = self.current_solving_batch()?;
        if batch_id != verify_batch_id {
            info!(
                "batch ID changed during run loop ({} -> {}); retrying",
                batch_id, verify_batch_id,
            );
            return Ok(());
        }

        let current_batch_time = BATCH_DURATION - time_remaining;
        if current_batch_time > self.config.solver_time_limit {
            // TODO(nlordell): This should probably be reflected in a metric.
            //   For now we just log an warning.
            warn!("skipping batch {}", batch_id);
            return Ok(());
        }

        let time_limit = self.config.solver_time_limit - current_batch_time;

        info!(
            "solving for batch {} with time limit {}s",
            batch_id,
            time_limit.as_secs_f64(),
        );
        match self.driver.run(batch_id, time_limit).wait() {
            DriverResult::Ok => {
                info!("successfully solved batch {}", batch_id);
                self.last_batch = Some(batch_id);
            }
            DriverResult::Retry(err) => {
                error!("driver retryable error for batch {}: {}", batch_id, err);
            }
            DriverResult::Skip(err) => {
                error!("driver error for batch {}: {}", batch_id, err);
                self.last_batch = Some(batch_id);
            }
        }

        Ok(())
    }

    /// Gets the current solving batch ID.
    ///
    /// This is the current batch ID minus 1, as the current batch ID is the ID
    /// of the batch that is currently accepting orders.
    fn current_solving_batch(&self) -> Result<u32> {
        Ok(self.exchange.get_current_auction_index().wait()? - 1)
    }
}

impl Scheduler for EvmScheduler<'_> {
    fn start(&mut self) -> ! {
        loop {
            if let Err(err) = self.step() {
                error!("EVM scheduler error: {:?}", err);
            }
            thread::sleep(POLL_TIMEOUT);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::contracts::stablex_contract::MockStableXContract;
    use crate::driver::stablex_driver::MockStableXDriver;
    use anyhow::anyhow;
    use futures::future::FutureExt as _;
    use mockall::predicate::eq;
    use mockall::Sequence;

    #[test]
    fn scheduler_step() {
        let mut exchange = MockStableXContract::new();
        exchange
            .expect_get_current_auction_index()
            .returning(|| async { Ok(42) }.boxed());
        exchange
            .expect_get_current_auction_remaining_time()
            .returning(|| async { Ok(Duration::from_secs(270)) }.boxed());

        let mut driver = MockStableXDriver::new();
        driver
            .expect_run()
            .with(eq(41), eq(Duration::from_secs(150)))
            .returning(|_, _| async { DriverResult::Ok }.boxed());

        let mut scheduler = EvmScheduler::with_defaults(&exchange, &driver);

        scheduler.step().unwrap();
        assert_eq!(scheduler.last_batch, Some(41));
    }

    #[test]
    fn scheduler_run_next_batch() {
        let mut exchange = MockStableXContract::new();
        exchange
            .expect_get_current_auction_index()
            .returning(|| async { Ok(42) }.boxed());
        exchange
            .expect_get_current_auction_remaining_time()
            .returning(|| async { Ok(Duration::from_secs(240)) }.boxed());

        let mut driver = MockStableXDriver::new();
        driver
            .expect_run()
            .returning(|_, _| async { DriverResult::Ok }.boxed());

        let mut scheduler = EvmScheduler::with_defaults(&exchange, &driver);
        scheduler.last_batch = Some(40);

        scheduler.step().unwrap();
        assert_eq!(scheduler.last_batch, Some(41));
    }

    #[test]
    fn scheduler_skips_previously_processed_batches() {
        let mut exchange = MockStableXContract::new();
        exchange
            .expect_get_current_auction_index()
            .returning(|| async { Ok(42) }.boxed());

        let driver = MockStableXDriver::new();

        let mut scheduler = EvmScheduler::with_defaults(&exchange, &driver);
        scheduler.last_batch = Some(41);

        scheduler.step().unwrap();
    }

    #[test]
    fn scheduler_retries_when_batch_changes_during_run() {
        let mut seq = Sequence::new();
        let mut exchange = MockStableXContract::new();
        exchange
            .expect_get_current_auction_index()
            .times(1)
            .in_sequence(&mut seq)
            .returning(|| async { Ok(42) }.boxed());
        exchange
            .expect_get_current_auction_remaining_time()
            .returning(|| async { Ok(Duration::from_secs(270)) }.boxed());
        exchange
            .expect_get_current_auction_index()
            .times(1)
            .in_sequence(&mut seq)
            .returning(|| async { Ok(43) }.boxed());

        let driver = MockStableXDriver::new();

        let mut scheduler = EvmScheduler::with_defaults(&exchange, &driver);

        scheduler.step().unwrap();
        assert_eq!(scheduler.last_batch, None);
    }

    #[test]
    fn scheduler_skips_batches_without_enough_time() {
        let mut exchange = MockStableXContract::new();
        exchange
            .expect_get_current_auction_index()
            .returning(|| async { Ok(42) }.boxed());
        exchange
            .expect_get_current_auction_remaining_time()
            .returning(|| async { Ok(Duration::from_secs(1)) }.boxed());

        let driver = MockStableXDriver::new();

        let mut scheduler = EvmScheduler::with_defaults(&exchange, &driver);

        scheduler.step().unwrap();
        assert_eq!(scheduler.last_batch, None);
    }

    #[test]
    fn scheduler_updates_last_batch_on_hard_driver_error() {
        let mut exchange = MockStableXContract::new();
        exchange
            .expect_get_current_auction_index()
            .returning(|| async { Ok(42) }.boxed());
        exchange
            .expect_get_current_auction_remaining_time()
            .returning(|| async { Ok(Duration::from_secs(270)) }.boxed());

        let mut driver = MockStableXDriver::new();
        driver
            .expect_run()
            .returning(|_, _| async { DriverResult::Skip(anyhow!("error")) }.boxed());

        let mut scheduler = EvmScheduler::with_defaults(&exchange, &driver);

        scheduler.step().unwrap();
        assert_eq!(scheduler.last_batch, Some(41));
    }

    #[test]
    fn scheduler_does_not_update_last_batch_on_retryable_driver_error() {
        let mut exchange = MockStableXContract::new();
        exchange
            .expect_get_current_auction_index()
            .returning(|| async { Ok(42) }.boxed());
        exchange
            .expect_get_current_auction_remaining_time()
            .returning(|| async { Ok(Duration::from_secs(270)) }.boxed());

        let mut driver = MockStableXDriver::new();
        driver
            .expect_run()
            .returning(|_, _| async { DriverResult::Retry(anyhow!("error")) }.boxed());

        let mut scheduler = EvmScheduler::with_defaults(&exchange, &driver);

        scheduler.step().unwrap();
        assert_eq!(scheduler.last_batch, None);
    }
}
