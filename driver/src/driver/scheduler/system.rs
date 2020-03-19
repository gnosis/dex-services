use super::{AuctionTimingConfiguration, Scheduler, BATCH_DURATION};
use crate::driver::stablex_driver::{DriverResult, StableXDriver};
use anyhow::{Context, Result};
use crossbeam_utils::thread::Scope;
use log::error;
use log::info;
use std::thread;
use std::time::{Duration, SystemTime, SystemTimeError};

const RETRY_SLEEP_DURATION: Duration = Duration::from_secs(10);

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

    fn start_solving_in_thread<'b>(
        &self,
        batch_id: BatchId,
        solver_time_limit: Duration,
        scope: &Scope<'b>,
    ) where
        'a: 'b,
    {
        let driver = self.driver;
        let auction_timing_configuration = self.auction_timing_configuration;
        scope.spawn(move |_| loop {
            let driver_result = driver.run(batch_id.0.into(), solver_time_limit);
            log_driver_result(batch_id, &driver_result);
            if should_attempt_to_solve_again(
                auction_timing_configuration,
                SystemTime::now(),
                batch_id,
                &driver_result,
            ) {
                thread::sleep(RETRY_SLEEP_DURATION);
            } else {
                break;
            }
        });
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

fn should_attempt_to_solve_again(
    auction_timing_configuration: AuctionTimingConfiguration,
    now: SystemTime,
    batch_id: BatchId,
    driver_result: &DriverResult,
) -> bool {
    match driver_result {
        DriverResult::Ok => false,
        DriverResult::Retry(_) => {
            now < (batch_id.solve_start_time() + auction_timing_configuration.solver_time_limit)
        }
        DriverResult::Skip(_) => false,
    }
}

fn log_driver_result(batch_id: BatchId, driver_result: &DriverResult) {
    match driver_result {
        DriverResult::Ok => info!("Batch {} solved without error.", batch_id.0),
        DriverResult::Retry(err) => {
            error!("Batch {} failed with retryable error: {}", batch_id.0, err)
        }
        DriverResult::Skip(err) => error!(
            "Batch {} failed with unretryable error: {}",
            batch_id.0, err
        ),
    }
}

impl<'a> Scheduler for SystemScheduler<'a> {
    fn start(&mut self) -> ! {
        crossbeam_utils::thread::scope(|scope| -> ! {
            loop {
                match self.determine_action(SystemTime::now()) {
                    Ok(Action::Sleep(duration)) => {
                        info!("Sleeping {}s.", duration.as_secs());
                        thread::sleep(duration);
                    }
                    Ok(Action::Solve(batch_id, duration)) => {
                        info!("Starting to solve batch {}.", batch_id.0);
                        self.last_solved_batch = Some(batch_id);
                        self.start_solving_in_thread(batch_id, duration, scope)
                    }
                    Err(err) => {
                        error!("Scheduler error: {}", err);
                        thread::sleep(RETRY_SLEEP_DURATION);
                    }
                };
            }
        })
        .unwrap();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::driver::stablex_driver::MockStableXDriver;
    use anyhow::anyhow;

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
    fn should_attempt_to_solve_again_() {
        let auction_timing_configuration = AuctionTimingConfiguration {
            target_start_solve_time: Duration::from_secs(10),
            solver_time_limit: Duration::from_secs(20),
        };

        let base_time = SystemTime::UNIX_EPOCH + Duration::from_secs(300);

        assert_eq!(
            should_attempt_to_solve_again(
                auction_timing_configuration,
                base_time + Duration::from_secs(10),
                BatchId(0),
                &DriverResult::Ok
            ),
            false
        );

        assert_eq!(
            should_attempt_to_solve_again(
                auction_timing_configuration,
                base_time + Duration::from_secs(10),
                BatchId(0),
                &DriverResult::Skip(anyhow!("")),
            ),
            false
        );

        assert_eq!(
            should_attempt_to_solve_again(
                auction_timing_configuration,
                base_time + Duration::from_secs(10),
                BatchId(0),
                &DriverResult::Retry(anyhow!("")),
            ),
            true
        );

        assert_eq!(
            should_attempt_to_solve_again(
                auction_timing_configuration,
                base_time + Duration::from_secs(19),
                BatchId(0),
                &DriverResult::Retry(anyhow!("")),
            ),
            true
        );

        assert_eq!(
            should_attempt_to_solve_again(
                auction_timing_configuration,
                base_time + Duration::from_secs(20),
                BatchId(0),
                &DriverResult::Retry(anyhow!("")),
            ),
            false
        );

        assert_eq!(
            should_attempt_to_solve_again(
                auction_timing_configuration,
                base_time + Duration::from_secs(310),
                BatchId(0),
                &DriverResult::Retry(anyhow!("")),
            ),
            false
        );

        assert_eq!(
            should_attempt_to_solve_again(
                auction_timing_configuration,
                base_time + Duration::from_secs(310),
                BatchId(1),
                &DriverResult::Retry(anyhow!("")),
            ),
            true
        );
    }

    // Allows observing real behavior by looking at the log output.
    #[test]
    #[ignore]
    fn test_real() {
        use crate::driver::stablex_driver::DriverResult;
        let (_, _guard) = crate::logging::init("info");

        let mut driver = MockStableXDriver::new();

        let mut counter = 0;
        driver.expect_run().returning(move |batch, time_limit| {
            log::info!(
                "driver run called for the {}. time with batch {} time_limit {}",
                counter,
                batch.low_u64(),
                time_limit.as_secs(),
            );
            counter += 1;
            match counter % 3 {
                0 => DriverResult::Ok,
                1 => DriverResult::Retry(anyhow!("")),
                2 => DriverResult::Skip(anyhow!("")),
                _ => unreachable!(),
            }
        });

        let auction_timing_configuration = AuctionTimingConfiguration {
            target_start_solve_time: Duration::from_secs(10),
            solver_time_limit: Duration::from_secs(290),
        };
        let mut scheduler = SystemScheduler::new(&driver, auction_timing_configuration);

        scheduler.start();
    }
}
