//! Implementation of an EVM-based scheduler that retrieves current batch and
//! batch duration information directly from the EVM instead of system time.

use super::{AuctionTimingConfiguration, Scheduler};
use crate::{
    contracts::stablex_contract::StableXContract,
    driver::stablex_driver::{DriverError, StableXDriver},
    health::HealthReporting,
    models::batch_id::BATCH_DURATION,
    models::Solution,
    util::{AsyncSleep, AsyncSleeping, FutureWaitExt as _},
};
use anyhow::Result;
use log::{error, info, warn};
use std::{sync::Arc, thread, time::Duration};

/// The amount of time the scheduler should wait between polling.
const POLL_TIMEOUT: Duration = Duration::from_secs(5);
/// The amount of time to wait between retries after errors.
const RETRY_SLEEP_DURATION: Duration = Duration::from_secs(5);

/// An EVM-based scheduler for the exchange driver.
pub struct EvmScheduler {
    exchange: Arc<dyn StableXContract>,
    driver: Arc<dyn StableXDriver>,
    config: AuctionTimingConfiguration,
    sleep: Box<dyn AsyncSleeping>,
    health: Arc<dyn HealthReporting>,
}

impl EvmScheduler {
    /// Creates a new EVM-based scheduler from a configuration.
    pub fn new(
        exchange: Arc<dyn StableXContract>,
        driver: Arc<dyn StableXDriver>,
        health: Arc<dyn HealthReporting>,
        config: AuctionTimingConfiguration,
    ) -> Self {
        EvmScheduler {
            driver,
            exchange,
            config,
            sleep: Box::new(AsyncSleep),
            health,
        }
    }

    /// Creates a new scheduler with the default configuration.
    #[cfg(test)]
    pub fn with_defaults_and_sleep(
        exchange: Arc<dyn StableXContract>,
        driver: Arc<dyn StableXDriver>,
        sleep: Box<dyn AsyncSleeping>,
        health: Arc<dyn HealthReporting>,
    ) -> Self {
        EvmScheduler {
            driver,
            exchange,
            config: AuctionTimingConfiguration::default(),
            sleep,
            health,
        }
    }

    /// Gets the current solving batch ID.
    ///
    /// This is the current batch ID minus 1, as the current batch ID is the ID
    /// of the batch that is currently accepting orders.
    async fn current_solving_batch(&self) -> Result<u32> {
        Ok(self.exchange.get_current_auction_index().await? - 1)
    }

    async fn wait_for_batch_to_change(&self, batch: u32) -> Result<u32> {
        loop {
            let current_batch = self.current_solving_batch().await?;
            if current_batch != batch {
                return Ok(current_batch);
            }
            self.sleep.sleep(POLL_TIMEOUT).await;
        }
    }

    async fn batch_time(&self, batch_id: u32) -> Result<Option<Duration>> {
        let time_remaining = self.exchange.get_current_auction_remaining_time().await?;
        // Without this check it would be appear as if the the time remaining increased when the
        // batch changes.
        if self.current_solving_batch().await? != batch_id {
            return Ok(None);
        }
        let batch_time = BATCH_DURATION
            .checked_sub(time_remaining)
            .expect("time remaining greater than batch duration");
        Ok(Some(batch_time))
    }

    async fn solver_time_limit(&self, batch_id: u32) -> Result<Option<Duration>> {
        let batch_time = self.batch_time(batch_id).await?;
        let time_limit = batch_time.and_then(|batch_time| {
            self.config
                .latest_solution_submit_time
                .checked_sub(batch_time)
        });
        Ok(time_limit)
    }

    async fn solve(&self, batch_id: u32) -> Result<Option<Solution>> {
        while let Some(time_limit) = self.solver_time_limit(batch_id).await? {
            info!(
                "solving for batch {} with time limit {}s",
                batch_id,
                time_limit.as_secs_f64(),
            );
            match self.driver.solve_batch(batch_id.into(), time_limit).await {
                Ok(solution) => {
                    info!("successfully solved batch {}", batch_id);
                    return Ok(Some(solution));
                }
                Err(DriverError::Retry(err)) => {
                    error!("driver retryable error for batch {}: {:?}", batch_id, err);
                }
                Err(DriverError::Skip(err)) => {
                    error!("driver error for batch {}: {:?}", batch_id, err);
                    return Ok(None);
                }
            }
        }
        warn!("skipping batch {} because we ran out of time", batch_id);
        Ok(None)
    }

    async fn submit(&self, batch_id: u32, solution: Solution) -> Result<()> {
        while match self.batch_time(batch_id).await? {
            None => {
                warn!("batch changed while waiting for earliest solution submit time");
                return Ok(());
            }
            Some(duration) => duration < self.config.earliest_solution_submit_time,
        } {
            self.sleep.sleep(POLL_TIMEOUT).await;
        }

        match self.driver.submit_solution(batch_id.into(), solution).await {
            Ok(()) => info!("successfully submitted solution for batch {}", batch_id),
            Err(err) => error!(
                "failed to submit solution for batch {}: {:?}",
                batch_id, err
            ),
        }
        Ok(())
    }

    /// Wait for and solve the next batch.
    async fn step(&self, last_batch: Option<u32>) -> Result<u32> {
        let last_batch = match last_batch {
            None => self.current_solving_batch().await?,
            Some(batch) => batch,
        };
        let new_batch = self.wait_for_batch_to_change(last_batch).await?;
        self.health.notify_ready();
        let solution = self.solve(new_batch).await?;
        if let Some(solution) = solution {
            self.submit(new_batch, solution).await?;
        }
        Ok(new_batch)
    }
}

impl Scheduler for EvmScheduler {
    fn start(&mut self) -> ! {
        let mut previous_batch = None;
        loop {
            match self.step(previous_batch).wait() {
                Ok(batch) => previous_batch = Some(batch),
                Err(err) => error!("EVM scheduler error: {:?}", err),
            }
            thread::sleep(RETRY_SLEEP_DURATION);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        contracts::stablex_contract::MockStableXContract,
        driver::stablex_driver::MockStableXDriver,
        health::MockHealthReporting,
        models::{BatchId, Solution},
        util::MockAsyncSleeping,
    };
    use anyhow::anyhow;
    use futures::future::FutureExt as _;
    use mockall::{predicate::eq, Sequence};

    #[test]
    fn scheduler_first_step_waits_for_second_batch_and_reports_healthy() {
        let mut sequence = Sequence::new();
        let mut exchange = MockStableXContract::new();
        let mut health = MockHealthReporting::new();
        exchange
            .expect_get_current_auction_index()
            .times(1)
            .in_sequence(&mut sequence)
            .returning(|| Ok(42));
        exchange
            .expect_get_current_auction_index()
            .times(1)
            .in_sequence(&mut sequence)
            .returning(|| Ok(43));
        health
            .expect_notify_ready()
            .times(1)
            .in_sequence(&mut sequence)
            .returning(|| ());

        exchange
            .expect_get_current_auction_index()
            .returning(|| Ok(43));
        exchange
            .expect_get_current_auction_remaining_time()
            .returning(|| Ok(Duration::from_secs(270)));

        let mut driver = MockStableXDriver::new();
        driver
            .expect_solve_batch()
            .with(eq(BatchId(42)), eq(Duration::from_secs(150)))
            .returning(|_, _| Err(DriverError::Skip(anyhow!(""))));

        let mut sleep = Box::new(MockAsyncSleeping::new());
        sleep.expect_sleep().returning(|_| immediate!(()));

        let scheduler = EvmScheduler::with_defaults_and_sleep(
            Arc::new(exchange),
            Arc::new(driver),
            sleep,
            Arc::new(health),
        );

        let result = scheduler.step(None).now_or_never().unwrap().unwrap();
        assert_eq!(result, 42);
    }

    #[test]
    fn scheduler_only_runs_next_batch_after_previous_has_finished() {
        let mut exchange = MockStableXContract::new();
        exchange
            .expect_get_current_auction_index()
            .times(2)
            .returning(|| Ok(41));
        exchange
            .expect_get_current_auction_index()
            .returning(|| Ok(42));
        exchange
            .expect_get_current_auction_remaining_time()
            .returning(|| Ok(Duration::from_secs(240)));

        let mut driver = MockStableXDriver::new();
        driver
            .expect_solve_batch()
            .returning(|_, _| Ok(Solution::trivial()));
        driver.expect_submit_solution().returning(|_, _| Ok(()));

        let mut sleep = Box::new(MockAsyncSleeping::new());
        sleep.expect_sleep().returning(|_| immediate!(()));

        let mut health = MockHealthReporting::new();
        health.expect_notify_ready().returning(|| ());

        let scheduler = EvmScheduler::with_defaults_and_sleep(
            Arc::new(exchange),
            Arc::new(driver),
            sleep,
            Arc::new(health),
        );

        let result = scheduler.step(Some(40)).now_or_never().unwrap().unwrap();
        assert_eq!(result, 41);
    }

    #[test]
    fn scheduler_skips_when_batch_changes_during_run() {
        let mut exchange = MockStableXContract::new();
        exchange
            .expect_get_current_auction_index()
            .times(1)
            .returning(|| Ok(42));
        exchange
            .expect_get_current_auction_remaining_time()
            .returning(|| Ok(Duration::from_secs(270)));
        exchange
            .expect_get_current_auction_index()
            .times(1)
            .returning(|| Ok(43));

        let driver = MockStableXDriver::new();

        let mut sleep = Box::new(MockAsyncSleeping::new());
        sleep.expect_sleep().returning(|_| immediate!(()));

        let mut health = MockHealthReporting::new();
        health.expect_notify_ready().returning(|| ());

        let scheduler = EvmScheduler::with_defaults_and_sleep(
            Arc::new(exchange),
            Arc::new(driver),
            sleep,
            Arc::new(health),
        );

        let result = scheduler.step(Some(40)).now_or_never().unwrap().unwrap();
        assert_eq!(result, 41);
    }

    #[test]
    fn scheduler_skips_batches_without_enough_time() {
        let mut exchange = MockStableXContract::new();
        exchange
            .expect_get_current_auction_index()
            .returning(|| Ok(42));
        exchange
            .expect_get_current_auction_remaining_time()
            .returning(|| Ok(Duration::from_secs(1)));

        let driver = MockStableXDriver::new();

        let mut sleep = Box::new(MockAsyncSleeping::new());
        sleep.expect_sleep().returning(|_| immediate!(()));

        let mut health = MockHealthReporting::new();
        health.expect_notify_ready().returning(|| ());

        let scheduler = EvmScheduler::with_defaults_and_sleep(
            Arc::new(exchange),
            Arc::new(driver),
            sleep,
            Arc::new(health),
        );

        let result = scheduler.step(Some(40)).now_or_never().unwrap().unwrap();
        assert_eq!(result, 41);
    }

    #[test]
    fn scheduler_retries_on_retryable_errors() {
        let mut exchange = MockStableXContract::new();
        exchange
            .expect_get_current_auction_index()
            .returning(|| Ok(42));
        exchange
            .expect_get_current_auction_remaining_time()
            .returning(|| Ok(Duration::from_secs(270)));

        let mut driver = MockStableXDriver::new();
        driver
            .expect_solve_batch()
            .times(1)
            .returning(|_, _| Err(DriverError::Retry(anyhow!("error"))));
        driver
            .expect_solve_batch()
            .times(1)
            .returning(|_, _| Ok(Solution::trivial()));
        driver.expect_submit_solution().returning(|_, _| Ok(()));

        let mut sleep = Box::new(MockAsyncSleeping::new());
        sleep.expect_sleep().returning(|_| immediate!(()));

        let mut health = MockHealthReporting::new();
        health.expect_notify_ready().returning(|| ());

        let scheduler = EvmScheduler::with_defaults_and_sleep(
            Arc::new(exchange),
            Arc::new(driver),
            sleep,
            Arc::new(health),
        );

        let result = scheduler.step(Some(40)).now_or_never().unwrap().unwrap();
        assert_eq!(result, 41);
    }

    #[test]
    fn scheduler_updates_last_batch_on_hard_driver_error() {
        let mut exchange = MockStableXContract::new();
        exchange
            .expect_get_current_auction_index()
            .returning(|| Ok(42));
        exchange
            .expect_get_current_auction_remaining_time()
            .returning(|| Ok(Duration::from_secs(270)));

        let mut driver = MockStableXDriver::new();
        driver
            .expect_solve_batch()
            .returning(|_, _| Err(DriverError::Skip(anyhow!("error"))));

        let mut sleep = Box::new(MockAsyncSleeping::new());
        sleep.expect_sleep().returning(|_| immediate!(()));

        let mut health = MockHealthReporting::new();
        health.expect_notify_ready().returning(|| ());

        let scheduler = EvmScheduler::with_defaults_and_sleep(
            Arc::new(exchange),
            Arc::new(driver),
            sleep,
            Arc::new(health),
        );

        let result = scheduler.step(Some(40)).now_or_never().unwrap().unwrap();
        assert_eq!(result, 41);
    }

    #[test]
    fn waits_for_earliest_solution_submit_time() {
        let mut exchange = MockStableXContract::new();
        exchange
            .expect_get_current_auction_index()
            .returning(|| Ok(42));
        exchange
            .expect_get_current_auction_remaining_time()
            .times(2)
            .returning(|| Ok(Duration::from_secs(270)));
        exchange
            .expect_get_current_auction_remaining_time()
            .times(1)
            .returning(|| Ok(Duration::from_secs(250)));

        let mut driver = MockStableXDriver::new();
        driver
            .expect_solve_batch()
            .returning(|_, _| Ok(Solution::trivial()));
        driver
            .expect_submit_solution()
            .times(1)
            .returning(|_, _| Ok(()));

        let mut sleep = Box::new(MockAsyncSleeping::new());
        sleep.expect_sleep().returning(|_| immediate!(()));

        let mut health = MockHealthReporting::new();
        health.expect_notify_ready().returning(|| ());

        let mut scheduler = EvmScheduler::with_defaults_and_sleep(
            Arc::new(exchange),
            Arc::new(driver),
            sleep,
            Arc::new(health),
        );
        scheduler.config.earliest_solution_submit_time = Duration::from_secs(50);

        let result = scheduler.step(Some(40)).now_or_never().unwrap().unwrap();
        assert_eq!(result, 41);
    }
}
