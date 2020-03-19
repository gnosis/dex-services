use super::{AuctionTimingConfiguration, Scheduler, BATCH_DURATION};
use crate::driver::stablex_driver::{DriverResult, StableXDriver};
use anyhow::{Context, Result};
use log::error;
use log::info;
use std::thread;
use std::time::{Duration, SystemTime, SystemTimeError};

/// Wraps a batch id as in the smart contract to add functionality related to
/// the current time.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
struct BatchId(u64);

impl BatchId {
    fn current(now: SystemTime) -> std::result::Result<Self, SystemTimeError> {
        let time_since_epoch = now.duration_since(SystemTime::UNIX_EPOCH)?;
        Ok(Self(time_since_epoch.as_secs() / BATCH_DURATION.as_secs()))
    }

    fn currently_being_solved(now: SystemTime) -> std::result::Result<Self, SystemTimeError> {
        Self::current(now).map(|batch_id| batch_id.prev())
    }

    fn order_collection_start_time(self) -> SystemTime {
        SystemTime::UNIX_EPOCH + Duration::from_secs(self.0 * BATCH_DURATION.as_secs())
    }

    fn solve_start_time(self) -> SystemTime {
        self.order_collection_start_time() + BATCH_DURATION
    }

    fn next(self) -> BatchId {
        self.0.checked_add(1).map(BatchId).unwrap()
    }

    fn prev(self) -> BatchId {
        self.0.checked_sub(1).map(BatchId).unwrap()
    }
}

pub struct SystemScheduler<'a> {
    driver: &'a (dyn StableXDriver + Sync),
    auction_timing_configuration: AuctionTimingConfiguration,
    last_solved_batch: Option<BatchId>,
}

#[derive(Debug, Eq, PartialEq)]
enum Action {
    Solve(BatchId, Duration),
    Sleep(Duration),
}

impl<'a> SystemScheduler<'a> {
    pub fn new(
        driver: &'a (dyn StableXDriver + Sync),
        auction_timing_configuration: AuctionTimingConfiguration,
    ) -> Self {
        Self {
            driver,
            auction_timing_configuration,
            last_solved_batch: None,
        }
    }

    /// Returns how long to sleep before starting the next iteration.
    fn run_once(&mut self, now: SystemTime) -> Duration {
        const RETRY_TIMEOUT: Duration = Duration::from_secs(10);
        match self.determine_action(now) {
            Ok(Action::Sleep(duration)) => {
                info!("Sleeping {}s.", duration.as_secs());
                duration
            }
            Ok(Action::Solve(batch_id, time_limit)) => {
                info!(
                    "Starting to solve batch {} with time limit {}s.",
                    batch_id.0,
                    time_limit.as_secs_f64()
                );
                match self.driver.run(batch_id.0.into(), time_limit) {
                    DriverResult::Ok => {
                        info!("Batch {} solved successfully.", batch_id.0);
                        self.last_solved_batch.replace(batch_id);
                        Duration::from_secs(0)
                    }
                    DriverResult::Retry(err) => {
                        error!("StableXDriver retryable error: {}", err);
                        RETRY_TIMEOUT
                    }
                    DriverResult::Skip(err) => {
                        error!("StableXDriver unretryable error: {}", err);
                        self.last_solved_batch.replace(batch_id);
                        Duration::from_secs(0)
                    }
                }
            }
            Err(err) => {
                error!("Scheduler error: {}", err);
                RETRY_TIMEOUT
            }
        }
    }

    /// Return current batch id and what to do with it.
    fn determine_action(&self, now: SystemTime) -> Result<Action> {
        let solving_batch = BatchId::currently_being_solved(now)
            .context("failed to get batch id currently being solved")?;
        let intended_solve_start_time = solving_batch.solve_start_time()
            + self.auction_timing_configuration.target_start_solve_time;
        // unwrap here because this cannot fail because the `solving_batch`'s
        // start time is always before `now`.
        let elapsed_time = now
            .duration_since(solving_batch.solve_start_time())
            .unwrap();

        let action = if self.last_solved_batch == Some(solving_batch)
            || elapsed_time >= self.auction_timing_configuration.solver_time_limit
        {
            let next = solving_batch.next();
            let duration = (next.solve_start_time()
                + self.auction_timing_configuration.target_start_solve_time)
                .duration_since(now)
                .unwrap();
            Action::Sleep(duration)
        } else if now < intended_solve_start_time {
            let duration = intended_solve_start_time.duration_since(now).unwrap();
            Action::Sleep(duration)
        } else {
            let time_limit = self.auction_timing_configuration.solver_time_limit - elapsed_time;
            Action::Solve(solving_batch, time_limit)
        };

        Ok(action)
    }
}

impl Scheduler for SystemScheduler<'_> {
    fn start(&mut self) -> ! {
        loop {
            thread::sleep(self.run_once(SystemTime::now()));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::driver::stablex_driver::MockStableXDriver;
    use anyhow::anyhow;
    use ethcontract::U256;
    use mockall::predicate::eq;

    #[test]
    fn batch_id_current() {
        let start_time = SystemTime::UNIX_EPOCH;
        let batch_id = BatchId::current(start_time).unwrap();
        assert_eq!(batch_id.0, 0);
        assert_eq!(batch_id.order_collection_start_time(), start_time);

        let start_time = SystemTime::UNIX_EPOCH;
        let batch_id = BatchId::current(start_time + Duration::from_secs(299)).unwrap();
        assert_eq!(batch_id.0, 0);
        assert_eq!(batch_id.order_collection_start_time(), start_time);

        let start_time = SystemTime::UNIX_EPOCH + Duration::from_secs(300);
        let batch_id = BatchId::current(start_time).unwrap();
        assert_eq!(batch_id.0, 1);
        assert_eq!(batch_id.order_collection_start_time(), start_time);

        let start_time = SystemTime::UNIX_EPOCH + Duration::from_secs(300);
        let batch_id = BatchId::current(start_time + Duration::from_secs(299)).unwrap();
        assert_eq!(batch_id.0, 1);
        assert_eq!(batch_id.order_collection_start_time(), start_time);
    }

    #[test]
    fn determine_action_without_matching_last_solved_batch() {
        let driver = MockStableXDriver::new();
        let auction_timing_configuration = AuctionTimingConfiguration {
            target_start_solve_time: Duration::from_secs(10),
            solver_time_limit: Duration::from_secs(20),
        };
        let scheduler = SystemScheduler::new(&driver, auction_timing_configuration);

        let base_time = SystemTime::UNIX_EPOCH + Duration::from_secs(300);

        assert_eq!(
            scheduler.determine_action(base_time).unwrap(),
            Action::Sleep(Duration::from_secs(10))
        );

        assert_eq!(
            scheduler
                .determine_action(base_time + Duration::from_secs(9))
                .unwrap(),
            Action::Sleep(Duration::from_secs(1))
        );

        assert_eq!(
            scheduler
                .determine_action(base_time + Duration::from_secs(10))
                .unwrap(),
            Action::Solve(BatchId(0), Duration::from_secs(10))
        );

        assert_eq!(
            scheduler
                .determine_action(base_time + Duration::from_secs(19))
                .unwrap(),
            Action::Solve(BatchId(0), Duration::from_secs(1))
        );

        // Sleep because we are behind latest_solve_attempt_time:

        assert_eq!(
            scheduler
                .determine_action(base_time + Duration::from_secs(20))
                .unwrap(),
            Action::Sleep(Duration::from_secs(290))
        );

        assert_eq!(
            scheduler
                .determine_action(base_time + Duration::from_secs(299))
                .unwrap(),
            Action::Sleep(Duration::from_secs(11))
        );
    }

    #[test]
    fn determine_action_with_matching_last_solved_batch() {
        let driver = MockStableXDriver::new();
        let auction_timing_configuration = AuctionTimingConfiguration {
            target_start_solve_time: Duration::from_secs(10),
            solver_time_limit: Duration::from_secs(20),
        };
        let mut scheduler = SystemScheduler::new(&driver, auction_timing_configuration);
        scheduler.last_solved_batch = Some(BatchId(0));

        let base_time = SystemTime::UNIX_EPOCH + Duration::from_secs(300);

        assert_eq!(
            scheduler.determine_action(base_time).unwrap(),
            Action::Sleep(Duration::from_secs(310))
        );

        assert_eq!(
            scheduler
                .determine_action(base_time + Duration::from_secs(9))
                .unwrap(),
            Action::Sleep(Duration::from_secs(301))
        );

        assert_eq!(
            scheduler
                .determine_action(base_time + Duration::from_secs(10))
                .unwrap(),
            Action::Sleep(Duration::from_secs(300))
        );

        assert_eq!(
            scheduler
                .determine_action(base_time + Duration::from_secs(19))
                .unwrap(),
            Action::Sleep(Duration::from_secs(291))
        );

        assert_eq!(
            scheduler
                .determine_action(base_time + Duration::from_secs(21))
                .unwrap(),
            Action::Sleep(Duration::from_secs(289))
        );

        assert_eq!(
            scheduler
                .determine_action(base_time + Duration::from_secs(299))
                .unwrap(),
            Action::Sleep(Duration::from_secs(11))
        );
    }

    #[test]
    fn run_forever_single_iteration() {
        let mut driver = MockStableXDriver::new();
        driver
            .expect_run()
            .with(eq(U256::from(0)), eq(Duration::from_secs(10)))
            .returning(|_, _| DriverResult::Ok);
        driver
            .expect_run()
            .with(eq(U256::from(1)), eq(Duration::from_secs(10)))
            .returning(|_, _| DriverResult::Retry(anyhow!("")));
        driver
            .expect_run()
            .with(eq(U256::from(2)), eq(Duration::from_secs(10)))
            .returning(|_, _| DriverResult::Skip(anyhow!("")));

        let auction_timing_configuration = AuctionTimingConfiguration {
            target_start_solve_time: Duration::from_secs(10),
            solver_time_limit: Duration::from_secs(20),
        };
        let mut scheduler = SystemScheduler::new(&driver, auction_timing_configuration);

        let base_time = SystemTime::UNIX_EPOCH + Duration::from_secs(300);

        // Driver not invoked
        assert_eq!(scheduler.run_once(base_time), Duration::from_secs(10));

        // Ok
        assert_eq!(
            scheduler.run_once(base_time + Duration::from_secs(10)),
            Duration::from_secs(0)
        );

        // Batch already handled
        assert_eq!(
            scheduler.run_once(base_time + Duration::from_secs(10)),
            Duration::from_secs(300)
        );

        // Retry
        assert_eq!(
            scheduler.run_once(base_time + Duration::from_secs(310)),
            Duration::from_secs(10)
        );

        // Skip
        assert_eq!(
            scheduler.run_once(base_time + Duration::from_secs(610)),
            Duration::from_secs(0)
        );

        // Batch already handled
        assert_eq!(
            scheduler.run_once(base_time + Duration::from_secs(610)),
            Duration::from_secs(300)
        );
    }
}
